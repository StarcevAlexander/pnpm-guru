use crate::registry::RegistryIndex;
use node_semver::Version;
use std::collections::{BTreeMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub struct Skipped {
    pub name: String,
    pub range: String,
    pub reason: SkippedReason,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkippedReason {
    UnknownPackage,
    InvalidRange(String),
    NoMatchingVersion,
    WorkspaceProtocol,
    OptionalNotSatisfiable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PeerRequest {
    pub name: String,
    pub range: String,
    pub required_by: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub name: String,
    pub kept_version: String,
    pub rejected_version: String,
    pub requested_range: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceLink {
    pub name: String,
    pub version: String,
    pub specifier: String,
    pub required_by: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolution {
    pub packages: BTreeMap<String, String>,
    pub conflicts: Vec<Conflict>,
    pub skipped: Vec<Skipped>,
    pub peer_requests: Vec<PeerRequest>,
    pub workspace_links: Vec<WorkspaceLink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DepKind {
    Required,
    Optional,
}

pub fn resolve(root_dependencies: &[(String, String)], index: &RegistryIndex) -> Resolution {
    resolve_inner(root_dependencies, index, &BTreeMap::new())
}

pub fn resolve_with_workspaces(
    root_dependencies: &[(String, String)],
    index: &RegistryIndex,
    workspace_versions: &BTreeMap<String, String>,
) -> Resolution {
    resolve_inner(root_dependencies, index, workspace_versions)
}

fn resolve_inner(
    root_dependencies: &[(String, String)],
    index: &RegistryIndex,
    workspace_versions: &BTreeMap<String, String>,
) -> Resolution {
    let mut resolution = Resolution::default();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut resolved: BTreeMap<String, String> = BTreeMap::new();
    let mut queue: VecDeque<(String, String, DepKind)> = VecDeque::new();

    for (name, range) in root_dependencies {
        queue.push_back((name.clone(), range.clone(), DepKind::Required));
    }

    while let Some((name, range, kind)) = queue.pop_front() {
        if !seen.insert((name.clone(), range.clone())) {
            continue;
        }

        if let Some(inner) = range.strip_prefix("workspace:") {
            if let Some(version) = workspace_versions.get(&name) {
                resolution.workspace_links.push(WorkspaceLink {
                    name: name.clone(),
                    version: version.clone(),
                    specifier: inner.to_string(),
                    required_by: String::from("<workspace>"),
                });
            } else {
                resolution.skipped.push(Skipped {
                    name: name.clone(),
                    range: range.clone(),
                    reason: SkippedReason::WorkspaceProtocol,
                });
            }
            continue;
        }

        let Some(entries) = index.packages.get(&name) else {
            let reason = match kind {
                DepKind::Optional => SkippedReason::OptionalNotSatisfiable,
                _ => SkippedReason::UnknownPackage,
            };
            resolution.skipped.push(Skipped {
                name: name.clone(),
                range: range.clone(),
                reason,
            });
            continue;
        };

        let requirement = match node_semver::Range::parse(&range) {
            Ok(req) => req,
            Err(e) => {
                resolution.skipped.push(Skipped {
                    name: name.clone(),
                    range: range.clone(),
                    reason: SkippedReason::InvalidRange(e.to_string()),
                });
                continue;
            }
        };

        let mut candidates: Vec<&Version> = entries
            .values()
            .map(|e| &e.version)
            .filter(|v| requirement.satisfies(v))
            .collect();
        candidates.sort();
        let Some(chosen) = candidates.pop().map(|v| v.to_string()) else {
            let reason = match kind {
                DepKind::Optional => SkippedReason::OptionalNotSatisfiable,
                _ => SkippedReason::NoMatchingVersion,
            };
            resolution.skipped.push(Skipped {
                name: name.clone(),
                range: range.clone(),
                reason,
            });
            continue;
        };

        if let Some(existing) = resolved.get(&name) {
            if existing != &chosen {
                resolution.conflicts.push(Conflict {
                    name: name.clone(),
                    kept_version: existing.clone(),
                    rejected_version: chosen.clone(),
                    requested_range: range.clone(),
                });
            }
            if let Some(entry) = index.packages.get(&name).and_then(|v| v.get(&chosen)) {
                for (dep_name, dep_range) in &entry.dependencies {
                    queue.push_back((dep_name.clone(), dep_range.clone(), DepKind::Required));
                }
            }
            continue;
        }

        resolved.insert(name.clone(), chosen.clone());
        let Some(entry) = index.packages.get(&name).and_then(|v| v.get(&chosen)) else {
            continue;
        };
        for (dep_name, dep_range) in &entry.dependencies {
            queue.push_back((dep_name.clone(), dep_range.clone(), DepKind::Required));
        }
        for (dep_name, dep_range) in &entry.optional_dependencies {
            queue.push_back((dep_name.clone(), dep_range.clone(), DepKind::Optional));
        }
        for (dep_name, dep_range) in &entry.peer_dependencies {
            resolution.peer_requests.push(PeerRequest {
                name: dep_name.clone(),
                range: dep_range.clone(),
                required_by: name.clone(),
            });
        }
    }

    resolution.packages = resolved;
    resolution
}
