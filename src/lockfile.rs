use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LOCKFILE_VERSION: &str = "9.0";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lockfile {
    #[serde(rename = "lockfileVersion")]
    pub lockfile_version: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub importers: BTreeMap<String, ImporterEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packages: BTreeMap<String, PackageRecord>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub snapshots: BTreeMap<String, SnapshotRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImporterEntry {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, DependencyEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dev_dependencies: BTreeMap<String, DependencyEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyEntry {
    pub specifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl DependencyEntry {
    pub fn new(specifier: impl Into<String>, version: Option<String>) -> Self {
        Self {
            specifier: specifier.into(),
            version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageRecord {
    pub resolution: ResolutionInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolutionInfo {
    pub integrity: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapshotRecord {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LockfileError {
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("unsupported lockfileVersion {0:?}, expected 9.0")]
    UnsupportedVersion(String),
}

pub fn parse_lockfile(yaml: &str) -> Result<Lockfile, LockfileError> {
    let lockfile: Lockfile = serde_yaml::from_str(yaml)?;
    if lockfile.lockfile_version != LOCKFILE_VERSION {
        return Err(LockfileError::UnsupportedVersion(lockfile.lockfile_version));
    }
    Ok(lockfile)
}

pub fn write_lockfile(lockfile: &Lockfile) -> Result<String, LockfileError> {
    Ok(serde_yaml::to_string(lockfile)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_roundtrip_keeps_specifiers() {
        let yaml = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      express:\n        specifier: ^4.18.0\n        version: 4.18.2\n";
        let lockfile = parse_lockfile(yaml).unwrap();
        assert_eq!(lockfile.lockfile_version, "9.0");
        let dep = &lockfile.importers["."].dependencies["express"];
        assert_eq!(dep.specifier, "^4.18.0");
        assert_eq!(dep.version.as_deref(), Some("4.18.2"));
    }

    #[test]
    fn unsupported_version_rejected() {
        let err = parse_lockfile("lockfileVersion: '6.0'").unwrap_err();
        assert!(err.to_string().contains("unsupported"));
    }

    #[test]
    fn write_produces_yaml() {
        let lockfile = Lockfile {
            lockfile_version: LOCKFILE_VERSION.to_string(),
            ..Lockfile::default()
        };
        let written = write_lockfile(&lockfile).unwrap();
        assert!(written.contains("lockfileVersion: '9.0'"));
    }
}
