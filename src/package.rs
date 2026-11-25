use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PackageJson {
    pub name: Option<String>,
    pub version: Option<String>,
    pub private: bool,
    pub dependencies: HashMap<String, String>,
    pub dev_dependencies: HashMap<String, String>,
    pub optional_dependencies: HashMap<String, String>,
    pub peer_dependencies: HashMap<String, String>,
    pub scripts: HashMap<String, String>,
    pub bin: Option<BinField>,
    pub workspaces: Option<Workspaces>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum BinField {
    Single(String),
    Map(HashMap<String, String>),
}

impl BinField {
    pub fn entries(&self, package_name: &str) -> Vec<(String, String)> {
        match self {
            BinField::Single(path) => vec![(package_name.to_string(), path.clone())],
            BinField::Map(map) => {
                let mut pairs: Vec<(String, String)> =
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                pairs.sort();
                pairs
            }
        }
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum Workspaces {
    List(Vec<String>),
    Config { packages: Vec<String> },
}

impl Workspaces {
    pub fn patterns(self) -> Vec<String> {
        match self {
            Workspaces::List(patterns) => patterns,
            Workspaces::Config { packages } => packages,
        }
    }
}

impl PackageJson {
    pub fn parse(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }

    pub fn production_dependencies(&self) -> Vec<(String, String)> {
        sorted(&self.dependencies)
    }

    pub fn dev_dependencies(&self) -> Vec<(String, String)> {
        sorted(&self.dev_dependencies)
    }

    pub fn optional_dependencies(&self) -> Vec<(String, String)> {
        sorted(&self.optional_dependencies)
    }

    pub fn peer_dependencies(&self) -> Vec<(String, String)> {
        sorted(&self.peer_dependencies)
    }
}

fn sorted(map: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> =
        map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    pairs.sort();
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_package_json() {
        let raw = r#"{
            "name": "my-app",
            "version": "1.2.3",
            "private": true,
            "dependencies": {"react": "^19.0.0"},
            "devDependencies": {"typescript": "^5.0.0"},
            "optionalDependencies": {"fsevents": "^2.3.3"},
            "peerDependencies": {"webpack": "^5.0.0"},
            "scripts": {"build": "tsc"},
            "workspaces": ["packages/*"]
        }"#;
        let package = PackageJson::parse(raw).unwrap();
        assert_eq!(package.name.as_deref(), Some("my-app"));
        assert_eq!(package.version.as_deref(), Some("1.2.3"));
        assert!(package.private);
        assert_eq!(package.dependencies["react"], "^19.0.0");
        assert_eq!(package.dev_dependencies["typescript"], "^5.0.0");
        assert_eq!(
            package.workspaces.unwrap().patterns(),
            vec!["packages/*".to_string()]
        );
    }

    #[test]
    fn workspaces_object_form_is_supported() {
        let raw = r#"{"workspaces": {"packages": ["apps/*", "libs/*"]}}"#;
        let package = PackageJson::parse(raw).unwrap();
        assert_eq!(
            package.workspaces.unwrap().patterns(),
            vec!["apps/*".to_string(), "libs/*".to_string()]
        );
    }

    #[test]
    fn empty_defaults() {
        let package = PackageJson::parse("{}").unwrap();
        assert!(package.dependencies.is_empty());
        assert!(!package.private);
    }

    #[test]
    fn sorted_dependencies_are_deterministic() {
        let raw = r#"{"dependencies": {"z": "1.0.0", "a": "2.0.0"}}"#;
        let package = PackageJson::parse(raw).unwrap();
        let deps = package.production_dependencies();
        assert_eq!(
            deps,
            vec![
                ("a".to_string(), "2.0.0".to_string()),
                ("z".to_string(), "1.0.0".to_string())
            ]
        );
    }
}
