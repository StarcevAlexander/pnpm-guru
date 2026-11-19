use pnpm_guru::{PackageJson, RegistryIndex, parse_version, resolve, satisfies};
use serde_json::json;

fn fixture_registry() -> RegistryIndex {
    let registry = json!({
        "express": {
            "4.16.4": {"dependencies": {"accepts": "~1.3.5"}},
            "4.17.1": {"dependencies": {"accepts": "~1.3.7"}},
            "4.18.2": {"dependencies": {"accepts": "~1.3.8"}}
        },
        "accepts": {
            "1.3.7": {"dependencies": {"mime": "1.4.1"}},
            "1.3.8": {}
        },
        "mime": {"1.4.1": {}},
        "left-pad": {"1.0.0": {}},
        "cyclic-a": {"1.0.0": {"dependencies": {"cyclic-b": "1.0.0"}}},
        "cyclic-b": {"1.0.0": {"dependencies": {"cyclic-a": "1.0.0"}}}
    });
    RegistryIndex::from_json(&registry)
}

#[test]
fn registry_index_parses_versions_and_dependencies() {
    let index = fixture_registry();
    let versions = index.versions_of("express");
    assert_eq!(versions.len(), 3);
    let entry = index.entry("express", "4.17.1").unwrap();
    assert_eq!(entry.dependencies["accepts"], "~1.3.7");
}

#[test]
fn semver_ranges_filter_versions() {
    let version = parse_version("4.17.1").unwrap();
    assert!(satisfies(&version, "^4.17.0"));
    assert!(!satisfies(&version, "^4.18.0"));
}

#[test]
fn resolve_picks_highest_satisfying_and_pulls_transitives() {
    let index = fixture_registry();
    let root = PackageJson::parse(r#"{"dependencies": {"express": "^4.16.0"}}"#).unwrap();
    let resolution = resolve(&root.production_dependencies(), &index);
    assert_eq!(resolution.packages["express"], "4.18.2");
    assert_eq!(resolution.packages["accepts"], "1.3.8");
    assert_eq!(resolution.packages.len(), 2);
    assert!(resolution.skipped.is_empty());
    assert!(resolution.conflicts.is_empty());
}

#[test]
fn transitives_follow_the_resolved_version_not_the_root_range() {
    let index = fixture_registry();
    let root = vec![("express".to_string(), "~4.17.1".to_string())];
    let resolution = resolve(&root, &index);
    assert_eq!(resolution.packages["express"], "4.17.1");
    assert_eq!(resolution.packages["accepts"], "1.3.8");
    assert_eq!(resolution.packages.len(), 2);
}

#[test]
fn unknown_package_is_skipped() {
    let index = fixture_registry();
    let root = PackageJson::parse(r#"{"dependencies": {"ghost": "^1.0.0"}}"#).unwrap();
    let resolution = resolve(&root.production_dependencies(), &index);
    assert!(resolution.packages.is_empty());
    assert_eq!(resolution.skipped[0].name, "ghost");
    assert!(matches!(
        resolution.skipped[0].reason,
        pnpm_guru::SkippedReason::UnknownPackage
    ));
}

#[test]
fn unsatisfiable_range_is_skipped() {
    let index = fixture_registry();
    let root = PackageJson::parse(r#"{"dependencies": {"express": "^5.0.0"}}"#).unwrap();
    let resolution = resolve(&root.production_dependencies(), &index);
    assert!(resolution.packages.is_empty());
    assert_eq!(resolution.skipped.len(), 1);
    assert!(matches!(
        resolution.skipped[0].reason,
        pnpm_guru::SkippedReason::NoMatchingVersion
    ));
}

#[test]
fn dependency_cycles_terminate() {
    let index = fixture_registry();
    let root = PackageJson::parse(r#"{"dependencies": {"cyclic-a": "1.0.0"}}"#).unwrap();
    let resolution = resolve(&root.production_dependencies(), &index);
    assert_eq!(resolution.packages.len(), 2);
    assert!(resolution.packages.contains_key("cyclic-a"));
    assert!(resolution.packages.contains_key("cyclic-b"));
    assert!(resolution.conflicts.is_empty());
}

#[test]
fn workspace_protocol_is_skipped() {
    let index = fixture_registry();
    let root = PackageJson::parse(r#"{"dependencies": {"my-lib": "workspace:*"}}"#).unwrap();
    let resolution = resolve(&root.production_dependencies(), &index);
    assert!(
        resolution
            .skipped
            .iter()
            .any(|s| s.reason == pnpm_guru::SkippedReason::WorkspaceProtocol)
    );
}

#[test]
fn peer_requests_are_collected() {
    let registry = json!({
        "plugin-host": {
            "1.0.0": {"peer_dependencies": {"react": "^18.0.0"}}
        }
    });
    let index = RegistryIndex::from_json(&registry);
    let root = PackageJson::parse(r#"{"dependencies": {"plugin-host": "1.0.0"}}"#).unwrap();
    let resolution = resolve(&root.production_dependencies(), &index);
    assert_eq!(resolution.peer_requests.len(), 1);
    assert_eq!(resolution.peer_requests[0].name, "react");
    assert_eq!(resolution.peer_requests[0].required_by, "plugin-host");
}

#[test]
fn first_resolved_version_wins_on_conflict() {
    let registry = json!({
        "app-core": {"1.0.0": {"dependencies": {"shared": "^1.0.0"}}},
        "app-ui": {"1.0.0": {"dependencies": {"shared": "^2.0.0"}}},
        "shared": {"1.5.0": {}, "2.5.0": {}}
    });
    let index = RegistryIndex::from_json(&registry);
    let root = vec![
        ("app-core".to_string(), "^1.0.0".to_string()),
        ("app-ui".to_string(), "^1.0.0".to_string()),
    ];
    let resolution = resolve(&root, &index);
    assert_eq!(resolution.packages["shared"], "1.5.0");
    assert_eq!(resolution.conflicts.len(), 1);
    assert_eq!(resolution.conflicts[0].name, "shared");
}
