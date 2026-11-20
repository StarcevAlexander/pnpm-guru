use pnpm_guru::{RegistryIndex, parse_lockfile, resolve_with_workspaces};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn workspace_deps_resolve_as_links() {
    let index = RegistryIndex::from_json(&json!({
        "express": {"4.18.2": {}}
    }));
    let root = vec![
        ("shared-utils".to_string(), "workspace:*".to_string()),
        ("express".to_string(), "^4.18.0".to_string()),
    ];
    let mut workspaces = BTreeMap::new();
    workspaces.insert("shared-utils".to_string(), "1.2.3".to_string());
    let resolution = resolve_with_workspaces(&root, &index, &workspaces);
    assert_eq!(resolution.workspace_links.len(), 1);
    assert_eq!(resolution.workspace_links[0].name, "shared-utils");
    assert_eq!(resolution.workspace_links[0].version, "1.2.3");
    assert_eq!(resolution.workspace_links[0].specifier, "*");
    assert_eq!(resolution.packages["express"], "4.18.2");
    assert!(!resolution.packages.contains_key("shared-utils"));
}

#[test]
fn unknown_workspace_name_is_skipped() {
    let index = RegistryIndex::from_json(&json!({}));
    let root = vec![("missing-ws".to_string(), "workspace:^1.0.0".to_string())];
    let mut workspaces = BTreeMap::new();
    workspaces.insert("other".to_string(), "1.0.0".to_string());
    let resolution = resolve_with_workspaces(&root, &index, &workspaces);
    assert!(resolution.workspace_links.is_empty());
    assert!(
        resolution
            .skipped
            .iter()
            .any(|s| s.reason == pnpm_guru::SkippedReason::WorkspaceProtocol)
    );
}

#[test]
fn registry_deps_still_resolve_alongside_workspaces() {
    let index = RegistryIndex::from_json(&json!({
        "express": {"4.18.2": {}},
        "accepts": {"1.3.8": {}}
    }));
    let root = vec![("express".to_string(), "^4.18.0".to_string())];
    let mut workspaces = BTreeMap::new();
    workspaces.insert("shared-utils".to_string(), "1.0.0".to_string());
    let resolution = resolve_with_workspaces(&root, &index, &workspaces);
    assert_eq!(resolution.packages["express"], "4.18.2");
}

#[test]
fn lockfile_roundtrip_preserves_importers() {
    let yaml = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      express:\n        specifier: ^4.18.0\n        version: 4.18.2\n  packages/app:\n    dependencies:\n      shared:\n        specifier: workspace:*\n        version: link:../shared\n";
    let lockfile = parse_lockfile(yaml).unwrap();
    assert!(lockfile.importers.contains_key("."));
    assert!(lockfile.importers.contains_key("packages/app"));
    let root_dep = &lockfile.importers["."].dependencies["express"];
    assert_eq!(root_dep.specifier, "^4.18.0");
    let ws_dep = &lockfile.importers["packages/app"].dependencies["shared"];
    assert_eq!(ws_dep.version.as_deref(), Some("link:../shared"));
}
