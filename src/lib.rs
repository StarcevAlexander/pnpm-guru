pub mod bins;
#[cfg(feature = "registry")]
pub mod client;
pub mod install;
pub mod lifecycle;
pub mod lockfile;
pub mod manifest;
pub mod node_modules;
pub mod package;
pub mod registry;
pub mod resolution;
pub mod semver_ext;
pub mod store;
pub mod tarball;
pub mod workspaces;

pub use bins::{BinError, bin_entries, link_bins};
#[cfg(feature = "registry")]
pub use client::{DEFAULT_REGISTRY, RegistryClient, RegistryError};
pub use install::{InstallError, InstallOptions, InstallReport, InstalledPackage, install};
pub use lifecycle::{
    DEPENDENCY_LIFECYCLE_SCRIPTS, ScriptError, lifecycle_scripts, package_env, run_script,
    shell_command,
};
pub use lockfile::{
    DependencyEntry, ImporterEntry, LOCKFILE_VERSION, Lockfile, LockfileError, PackageRecord,
    ResolutionInfo, SnapshotRecord, parse_lockfile, write_lockfile,
};
pub use manifest::{ManifestFile, PackageManifest};
pub use node_modules::{AssembleError, AssembledPackage, assemble, create_alias};
pub use package::PackageJson;
pub use registry::{PackageEntry, RegistryIndex};
pub use resolution::{
    Conflict, PeerRequest, Resolution, Skipped, SkippedReason, WorkspaceLink, resolve,
    resolve_with_workspaces,
};
pub use semver_ext::{parse_range, parse_version, satisfies};
pub use store::{ContentStore, sha256_hex};
pub use tarball::{TarballError, UnpackedTarball, ssri_sha512, unpack_tarball};
pub use workspaces::WorkspacePackage;
