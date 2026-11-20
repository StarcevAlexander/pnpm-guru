use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ManifestFile {
    pub path: String,
    pub hash: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub files: Vec<ManifestFile>,
}

impl PackageManifest {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            files: Vec::new(),
        }
    }

    pub fn add_file(mut self, path: impl Into<String>, hash: impl Into<String>, size: u64) -> Self {
        self.files.push(ManifestFile {
            path: path.into(),
            hash: hash.into(),
            size,
        });
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    pub fn from_json(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }
}
