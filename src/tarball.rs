use crate::manifest::PackageManifest;
use crate::package::PackageJson;
use crate::store::{ContentStore, StoreError};
use flate2::read::GzDecoder;
use sha2::Digest as _;
use std::io::Read as _;
use tar::Archive;

#[derive(Debug, thiserror::Error)]
pub enum TarballError {
    #[error("integrity mismatch: expected {expected}, got {actual}")]
    IntegrityMismatch { expected: String, actual: String },
    #[error("unsupported integrity algorithm in {raw:?}")]
    UnsupportedIntegrity { raw: String },
    #[error("tarball has no package.json")]
    NoPackageJson,
    #[error("invalid gzip/tar data: {0}")]
    Archive(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for ch in input.bytes() {
        if ch == b'=' || ch == b'\n' || ch == b'\r' {
            continue;
        }
        let value = alphabet.iter().position(|a| *a == ch)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
        }
    }
    Some(output)
}

fn base64_encode(input: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(alphabet[(triple >> 18) as usize & 0x3f] as char);
        out.push(alphabet[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            alphabet[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            alphabet[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

pub fn ssri_sha512(data: &[u8]) -> String {
    format!("sha512-{}", base64_encode(&sha2::Sha512::digest(data)))
}

pub fn verify_integrity(data: &[u8], integrity: &str) -> Result<(), TarballError> {
    let Some((algorithm, digest)) = integrity.split_once('-') else {
        return Err(TarballError::UnsupportedIntegrity {
            raw: integrity.to_string(),
        });
    };
    let actual = match algorithm {
        "sha512" => base64_encode(&sha2::Sha512::digest(data)),
        "sha256" => base64_encode(&sha2::Sha256::digest(data)),
        _ => {
            return Err(TarballError::UnsupportedIntegrity {
                raw: algorithm.to_string(),
            });
        }
    };
    if base64_decode(digest).is_none() {
        return Err(TarballError::UnsupportedIntegrity {
            raw: integrity.to_string(),
        });
    }
    let expected_normalized: String = digest.chars().filter(|c| !c.is_whitespace()).collect();
    if actual != expected_normalized {
        return Err(TarballError::IntegrityMismatch {
            expected: integrity.to_string(),
            actual: format!("{algorithm}-{actual}"),
        });
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct UnpackedTarball {
    pub manifest: PackageManifest,
    pub package_json: PackageJson,
    pub files: usize,
}

pub fn unpack_tarball(
    store: &ContentStore,
    gz: &[u8],
    integrity: Option<&str>,
    fallback_name: &str,
    fallback_version: &str,
) -> Result<UnpackedTarball, TarballError> {
    if let Some(expected) = integrity {
        verify_integrity(gz, expected)?;
    }
    let tar = GzDecoder::new(gz);
    let mut archive = Archive::new(tar);
    let mut manifest = PackageManifest::new(fallback_name, fallback_version);
    let mut package_json: Option<PackageJson> = None;
    let mut files = 0usize;
    for entry in archive
        .entries()
        .map_err(|e| TarballError::Archive(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| TarballError::Archive(e.to_string()))?;
        let raw_path = entry
            .path()
            .map_err(|e| TarballError::Archive(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let path = raw_path.strip_prefix("package/").unwrap_or(&raw_path);
        if path.is_empty() || path.ends_with('/') || path == "pax_global_header" {
            continue;
        }
        if entry.header().entry_type().is_symlink() {
            continue;
        }
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|e| TarballError::Archive(e.to_string()))?;
        if path == "package.json" {
            package_json = Some(PackageJson::parse(&String::from_utf8_lossy(&data))?);
        }
        let hash = store.put_bytes(&data)?;
        manifest = manifest.add_file(path.to_string(), hash, data.len() as u64);
        files += 1;
    }
    let package_json = package_json.ok_or(TarballError::NoPackageJson)?;
    if let Some(name) = &package_json.name {
        manifest.name = name.clone();
    }
    if let Some(version) = &package_json.version {
        manifest.version = version.clone();
    }
    Ok(UnpackedTarball {
        manifest,
        package_json,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn build_tgz(files: &[(&str, &str)]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("package/{path}"), content.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn unpacks_and_fills_store() {
        let tgz = build_tgz(&[
            ("package.json", r#"{"name":"left-pad","version":"1.3.0"}"#),
            ("index.js", "module.exports = () => 'x'.repeat(n)"),
        ]);
        let store = ContentStore::new(std::env::temp_dir().join("pnpm-guru-test-tarball"));
        let unpacked = unpack_tarball(&store, &tgz, None, "left-pad", "1.3.0").unwrap();
        assert_eq!(unpacked.manifest.name, "left-pad");
        assert_eq!(unpacked.manifest.version, "1.3.0");
        assert_eq!(unpacked.files, 2);
        assert_eq!(unpacked.manifest.files.len(), 2);
        assert_eq!(unpacked.package_json.name.as_deref(), Some("left-pad"));
        let index = unpacked
            .manifest
            .files
            .iter()
            .find(|f| f.path == "index.js")
            .unwrap();
        let stored = store.read(&index.hash).unwrap();
        assert_eq!(stored, b"module.exports = () => 'x'.repeat(n)");
    }

    #[test]
    fn integrity_sha512_is_verified() {
        let tgz = build_tgz(&[("package.json", r#"{"name":"a","version":"1.0.0"}"#)]);
        let good = ssri_sha512(&tgz);
        let store = ContentStore::new(std::env::temp_dir().join("pnpm-guru-test-tarball2"));
        unpack_tarball(&store, &tgz, Some(&good), "a", "1.0.0").unwrap();
        let err = unpack_tarball(&store, &tgz, Some("sha512-AAAA"), "a", "1.0.0");
        assert!(matches!(err, Err(TarballError::IntegrityMismatch { .. })));
    }

    #[test]
    fn missing_package_json_rejected() {
        let tgz = build_tgz(&[("index.js", "x")]);
        let store = ContentStore::new(std::env::temp_dir().join("pnpm-guru-test-tarball3"));
        let err = unpack_tarball(&store, &tgz, None, "a", "1.0.0").unwrap_err();
        assert!(matches!(err, TarballError::NoPackageJson));
    }

    #[test]
    fn base64_roundtrip() {
        assert_eq!(base64_decode(&base64_encode(b"hello")).unwrap(), b"hello");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }
}
