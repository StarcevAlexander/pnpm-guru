use crate::package::PackageJson;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct WorkspacePackage {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    pub package_json: PackageJson,
}

pub fn discover(root_dir: &Path, patterns: &[String]) -> Vec<WorkspacePackage> {
    let mut out = Vec::new();
    for pattern in patterns {
        let pattern_path = root_dir.join(pattern);
        let Some(parent) = pattern_path.parent() else {
            continue;
        };
        let Some(leaf) = pattern_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(parent) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(dirname) = dir.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !leaf_matches(leaf, dirname) {
                continue;
            }
            let manifest_path = dir.join("package.json");
            let Ok(raw) = fs_read(&manifest_path) else {
                continue;
            };
            let Ok(package) = PackageJson::parse(&raw) else {
                continue;
            };
            let Some(name) = package.name.clone() else {
                continue;
            };
            let version = package.version.clone().unwrap_or_else(|| "0.0.0".into());
            out.push(WorkspacePackage {
                name,
                version,
                dir: dir.clone(),
                package_json: package,
            });
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out.dedup_by(|a, b| a.dir == b.dir);
    out
}

fn fs_read(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

fn leaf_matches(glob: &str, name: &str) -> bool {
    if glob == "*" {
        return true;
    }
    if let Some(prefix) = glob.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    if let Some(suffix) = glob.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    name == glob
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_matcher_variants() {
        assert!(leaf_matches("*", "anything"));
        assert!(leaf_matches("pkg-*", "pkg-core"));
        assert!(!leaf_matches("pkg-*", "other"));
        assert!(leaf_matches("exact", "exact"));
        assert!(!leaf_matches("exact", "other"));
    }
}
