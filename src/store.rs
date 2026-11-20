use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("invalid hash {raw:?}: expected 64 hex characters")]
    InvalidHash { raw: String },
    #[error("integrity mismatch for {expected}: got {actual}")]
    IntegrityMismatch { expected: String, actual: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn is_valid_hash(raw: &str) -> bool {
    raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit())
}

pub struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    fn path_for(&self, hash: &str) -> Result<PathBuf, StoreError> {
        if !is_valid_hash(hash) {
            return Err(StoreError::InvalidHash {
                raw: hash.to_string(),
            });
        }
        Ok(self.files_dir().join(&hash[..2]).join(hash))
    }

    pub fn has(&self, hash: &str) -> bool {
        self.path_for(hash).map(|p| p.is_file()).unwrap_or(false)
    }

    pub fn put_bytes(&self, data: &[u8]) -> Result<String, StoreError> {
        let hash = sha256_hex(data);
        let target = self.path_for(&hash)?;
        if !target.exists() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&target, data)?;
        }
        Ok(hash)
    }

    pub fn put_file(&self, source: &Path) -> Result<String, StoreError> {
        let data = fs::read(source)?;
        self.put_bytes(&data)
    }

    pub fn read(&self, hash: &str) -> Result<Vec<u8>, StoreError> {
        let path = self.path_for(hash)?;
        let data = fs::read(path)?;
        let actual = sha256_hex(&data);
        if actual != hash {
            return Err(StoreError::IntegrityMismatch {
                expected: hash.to_string(),
                actual,
            });
        }
        Ok(data)
    }

    pub fn verify(&self, hash: &str) -> Result<(), StoreError> {
        let path = self.path_for(hash)?;
        if !path.is_file() {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("store file {hash} missing"),
            )));
        }
        let data = fs::read(path)?;
        let actual = sha256_hex(&data);
        if actual != hash {
            return Err(StoreError::IntegrityMismatch {
                expected: hash.to_string(),
                actual,
            });
        }
        Ok(())
    }
}

pub fn hard_link_or_copy(source: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::hard_link(source, target) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(source, target).map(|_| ()),
    }
}
