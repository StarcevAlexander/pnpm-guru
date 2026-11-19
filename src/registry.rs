use node_semver::Version;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct PackageEntry {
    pub version: Version,
    pub dependencies: BTreeMap<String, String>,
    pub optional_dependencies: BTreeMap<String, String>,
    pub peer_dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegistryIndex {
    pub(crate) packages: BTreeMap<String, BTreeMap<String, PackageEntry>>,
}

impl RegistryIndex {
    pub fn from_json(value: &Value) -> RegistryIndex {
        let mut packages: BTreeMap<String, BTreeMap<String, PackageEntry>> = BTreeMap::new();
        if let Some(root) = value.as_object() {
            for (name, versions) in root {
                let version_map = packages.entry(name.clone()).or_default();
                if let Some(version_map_src) = versions.as_object() {
                    for (version_raw, meta) in version_map_src {
                        let Ok(version) = Version::parse(version_raw) else {
                            continue;
                        };
                        let mut entry = PackageEntry {
                            version: version.clone(),
                            dependencies: BTreeMap::new(),
                            optional_dependencies: BTreeMap::new(),
                            peer_dependencies: BTreeMap::new(),
                        };
                        if let Some(meta_obj) = meta.as_object() {
                            for key in
                                ["dependencies", "optional_dependencies", "peer_dependencies"]
                            {
                                if let Some(deps) = meta_obj.get(key).and_then(|d| d.as_object()) {
                                    let target = match key {
                                        "dependencies" => &mut entry.dependencies,
                                        "optional_dependencies" => &mut entry.optional_dependencies,
                                        _ => &mut entry.peer_dependencies,
                                    };
                                    for (dep_name, dep_range) in deps {
                                        if let Some(range) = dep_range.as_str() {
                                            target.insert(dep_name.clone(), range.to_string());
                                        }
                                    }
                                }
                            }
                        }
                        version_map.insert(version_raw.clone(), entry);
                    }
                }
            }
        }
        RegistryIndex { packages }
    }

    pub fn versions_of(&self, name: &str) -> Vec<&Version> {
        self.packages
            .get(name)
            .map(|versions| versions.values().map(|e| &e.version).collect())
            .unwrap_or_default()
    }

    pub fn entry(&self, name: &str, version: &str) -> Option<&PackageEntry> {
        self.packages.get(name)?.get(version)
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    pub fn package_count(&self) -> usize {
        self.packages.len()
    }
}
