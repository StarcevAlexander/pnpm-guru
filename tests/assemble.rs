use pnpm_guru::{ContentStore, PackageManifest, assemble};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pnpm-guru-test-{tag}-{}-{nanos}",
        std::process::id()
    ));
    path
}

#[test]
fn store_roundtrip_is_content_addressed() {
    let store = ContentStore::new(temp_root("store"));
    let hash = store.put_bytes(b"hello pnpm").unwrap();
    assert_eq!(hash.len(), 64);
    assert!(store.has(&hash));
    assert_eq!(store.read(&hash).unwrap(), b"hello pnpm");
    let again = store.put_bytes(b"hello pnpm").unwrap();
    assert_eq!(again, hash);
}

#[test]
fn integrity_mismatch_is_detected() {
    let store = ContentStore::new(temp_root("integrity"));
    let hash = store.put_bytes(b"original").unwrap();
    let path = store.files_dir().join(&hash[..2]).join(&hash);
    fs::write(&path, b"tampered").unwrap();
    assert!(store.read(&hash).is_err());
    assert!(store.verify(&hash).is_err());
}

#[test]
fn assemble_links_files_and_creates_alias() {
    let base = temp_root("assemble");
    let store = ContentStore::new(base.join("store"));
    let mut manifest = PackageManifest::new("express", "4.18.2");
    for (path, content) in [
        (
            "package.json",
            "node_modules/express/package.json".as_bytes(),
        ),
        ("index.js", b"console.log('express');" as &[u8]),
    ] {
        let hash = store.put_bytes(content).unwrap();
        manifest = manifest.add_file(path, hash, content.len() as u64);
    }

    let target_root = base.join("project").join("node_modules");
    let assembled = assemble(&store, &manifest, &target_root).unwrap();

    assert!(assembled.real_dir.join("index.js").is_file());
    assert!(assembled.real_dir.join("package.json").is_file());
    assert_eq!(assembled.files_linked, 2);
    let alias_index = assembled.alias_dir.join("index.js");
    assert!(alias_index.is_file());
}

#[test]
fn manifest_json_roundtrip() {
    let manifest = PackageManifest::new("express", "4.18.2").add_file("index.js", "abc", 3);
    let raw = manifest.to_json();
    let back = PackageManifest::from_json(&raw).unwrap();
    assert_eq!(back, manifest);
}
