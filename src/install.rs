use crate::bins::link_bins;
#[cfg(feature = "registry")]
use crate::client::{DEFAULT_REGISTRY, RegistryClient};
use crate::lifecycle;
use crate::lockfile::{
    DependencyEntry, ImporterEntry, LOCKFILE_VERSION, Lockfile, PackageRecord, ResolutionInfo,
    SnapshotRecord, parse_lockfile, write_lockfile,
};
use crate::manifest::PackageManifest;
use crate::node_modules::{AssembleError, assemble};
use crate::package::PackageJson;
use crate::registry::RegistryIndex;
#[cfg(feature = "registry")]
use crate::resolution::resolve;
use crate::resolution::{Conflict, PeerRequest, Skipped, SkippedReason};
use crate::store::ContentStore;
use crate::tarball::TarballError;
#[cfg(feature = "registry")]
use crate::tarball::unpack_tarball;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub registry: String,
    pub production: bool,
    pub ignore_scripts: bool,
    pub offline: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            #[cfg(feature = "registry")]
            registry: DEFAULT_REGISTRY.to_string(),
            #[cfg(not(feature = "registry"))]
            registry: String::from("https://registry.npmjs.org"),
            production: false,
            ignore_scripts: false,
            offline: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub files: usize,
    pub bins: Vec<String>,
    pub scripts_run: Vec<String>,
}

#[derive(Debug, Default)]
pub struct InstallReport {
    pub installed: Vec<InstalledPackage>,
    pub skipped: Vec<Skipped>,
    pub conflicts: Vec<Conflict>,
    pub peer_requests: Vec<PeerRequest>,
    pub lockfile_written: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("lockfile error: {0}")]
    Lockfile(#[from] crate::lockfile::LockfileError),
    #[error("tarball error: {0}")]
    Tarball(#[from] TarballError),
    #[error("assemble error: {0}")]
    Assemble(#[from] AssembleError),
    #[error("bin error: {0}")]
    Bin(#[from] crate::bins::BinError),
    #[cfg(feature = "registry")]
    #[error("registry error: {0}")]
    Registry(#[from] crate::client::RegistryError),
    #[error(
        "network installs require the `registry` feature; use offline mode or enable the feature"
    )]
    RegistryDisabled,
    #[error("script error: {0}")]
    Script(#[from] crate::lifecycle::ScriptError),
    #[error("offline install needs a pnpm-lock.yaml")]
    OfflineNeedsLockfile,
    #[error("no stored manifest for {name}@{version}; run an online install first")]
    MissingManifest { name: String, version: String },
    #[error("packument for {name} has no tarball for {version}")]
    MissingTarball { name: String, version: String },
}

pub fn install(project: &Path, options: &InstallOptions) -> Result<InstallReport, InstallError> {
    let raw = fs::read_to_string(project.join("package.json"))?;
    let root = PackageJson::parse(&raw)?;
    let node_modules = project.join("node_modules");
    let store = ContentStore::new(node_modules.join(".pnpm-guru").join("store"));
    let manifests_dir = node_modules.join(".pnpm-guru").join("manifests");
    fs::create_dir_all(&manifests_dir)?;

    let mut dependencies = root.production_dependencies();
    if !options.production {
        dependencies.extend(root.dev_dependencies());
    }
    let dependencies = sorted_unique(dependencies);

    let mut report = InstallReport::default();

    let mut unsupported = Vec::new();
    let mut installable = Vec::new();
    for (name, range) in &dependencies {
        if range.starts_with("workspace:") {
            unsupported.push((
                name.clone(),
                range.clone(),
                SkippedReason::WorkspaceProtocol,
            ));
        } else if range.starts_with("file:") || range.starts_with("link:") {
            let protocol = range.split(':').next().unwrap_or("").to_string();
            unsupported.push((
                name.clone(),
                range.clone(),
                SkippedReason::UnsupportedProtocol(protocol),
            ));
        } else {
            installable.push((name.clone(), range.clone()));
        }
    }

    if !options.offline && !options.ignore_scripts {
        run_root_script(&root, "preinstall", project, &node_modules)?;
    }

    let (resolved, index) = if options.offline {
        resolve_offline(project, &installable, &mut report)?
    } else {
        #[cfg(feature = "registry")]
        {
            resolve_online(&options.registry, &installable, &mut report)?
        }
        #[cfg(not(feature = "registry"))]
        {
            return Err(InstallError::RegistryDisabled);
        }
    };

    let mut installed: Vec<InstalledPackage> = Vec::new();
    for (name, version) in &resolved {
        let manifest = if options.offline {
            load_manifest(&manifests_dir, name, version)?
        } else {
            #[cfg(feature = "registry")]
            {
                let entry =
                    index
                        .entry(name, version)
                        .ok_or_else(|| InstallError::MissingTarball {
                            name: name.clone(),
                            version: version.clone(),
                        })?;
                let tarball_url =
                    entry
                        .tarball
                        .clone()
                        .ok_or_else(|| InstallError::MissingTarball {
                            name: name.clone(),
                            version: version.clone(),
                        })?;
                let bytes = RegistryClient::new(&options.registry).tarball(&tarball_url)?;
                let unpacked =
                    unpack_tarball(&store, &bytes, entry.integrity.as_deref(), name, version)?;
                save_manifest(&manifests_dir, name, version, &unpacked.manifest)?;
                unpacked.manifest
            }
            #[cfg(not(feature = "registry"))]
            {
                return Err(InstallError::RegistryDisabled);
            }
        };
        let assembled = assemble(&store, &manifest, &node_modules)?;
        let installed_json = read_package_json(&assembled.real_dir)?;
        let scripts_run = if options.offline || options.ignore_scripts {
            Vec::new()
        } else {
            run_dependency_scripts(&installed_json, &assembled.real_dir, &node_modules)?
        };
        let bins = link_bins(&node_modules, &installed_json, &assembled.real_dir)?;
        installed.push(InstalledPackage {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            files: assembled.files_linked,
            bins,
            scripts_run,
        });
    }

    for (name, range, reason) in unsupported {
        report.skipped.push(Skipped {
            name,
            range,
            reason,
        });
    }
    report.installed = installed;

    let mut lockfile = build_lockfile(&resolved, &index);
    lockfile
        .importers
        .insert(String::from("."), importer_entry(&dependencies, &resolved));
    fs::write(project.join("pnpm-lock.yaml"), write_lockfile(&lockfile)?)?;
    report.lockfile_written = true;

    if !options.offline && !options.ignore_scripts {
        for script in ["install", "postinstall"] {
            run_root_script(&root, script, project, &node_modules)?;
        }
    }

    Ok(report)
}

fn run_root_script(
    root: &PackageJson,
    script: &str,
    project: &Path,
    node_modules: &Path,
) -> Result<(), InstallError> {
    if let Some(line) = root.scripts.get(script) {
        let name = root.name.as_deref().unwrap_or("<root>");
        let version = root.version.as_deref().unwrap_or("0.0.0");
        lifecycle::run_script(
            name,
            script,
            line,
            project,
            &lifecycle::package_env(name, version, project, node_modules),
        )?;
    }
    Ok(())
}

fn run_dependency_scripts(
    package_json: &PackageJson,
    real_dir: &Path,
    node_modules: &Path,
) -> Result<Vec<String>, InstallError> {
    let name = package_json
        .name
        .clone()
        .unwrap_or_else(|| String::from("<package>"));
    let version = package_json
        .version
        .clone()
        .unwrap_or_else(|| String::from("0.0.0"));
    let env = lifecycle::package_env(&name, &version, real_dir, node_modules);
    let mut run = Vec::new();
    for (script, line) in lifecycle::lifecycle_scripts(&package_json.scripts) {
        lifecycle::run_script(&name, &script, &line, real_dir, &env)?;
        run.push(script);
    }
    Ok(run)
}

#[cfg(feature = "registry")]
fn resolve_online(
    registry: &str,
    dependencies: &[(String, String)],
    report: &mut InstallReport,
) -> Result<(BTreeMap<String, String>, RegistryIndex), InstallError> {
    let client = RegistryClient::new(registry);
    let mut index = RegistryIndex::default();
    let mut fetched: BTreeSet<String> = BTreeSet::new();
    let resolution = loop {
        for (name, _) in dependencies {
            if fetched.insert(name.clone()) {
                index.extend(client.packument_index(name)?);
            }
        }
        let resolution = resolve(dependencies, &index);
        let missing: Vec<String> = resolution
            .skipped
            .iter()
            .filter(|skipped| matches!(skipped.reason, SkippedReason::UnknownPackage))
            .map(|skipped| skipped.name.clone())
            .filter(|name| fetched.insert(name.clone()))
            .collect();
        if missing.is_empty() {
            break resolution;
        }
        for name in missing {
            index.extend(client.packument_index(&name)?);
        }
    };
    report.skipped.extend(resolution.skipped.clone());
    report.conflicts.extend(resolution.conflicts.clone());
    report
        .peer_requests
        .extend(resolution.peer_requests.clone());
    Ok((resolution.packages, index))
}

fn resolve_offline(
    project: &Path,
    dependencies: &[(String, String)],
    report: &mut InstallReport,
) -> Result<(BTreeMap<String, String>, RegistryIndex), InstallError> {
    let raw = fs::read_to_string(project.join("pnpm-lock.yaml"))
        .map_err(|_| InstallError::OfflineNeedsLockfile)?;
    let lockfile = parse_lockfile(&raw)?;
    let importer = lockfile.importers.get(".").cloned().unwrap_or_default();
    let mut locked: BTreeMap<String, String> = BTreeMap::new();
    for entry in importer
        .dependencies
        .values()
        .chain(importer.dev_dependencies.values())
    {
        if let Some(version) = &entry.version {
            locked.insert(entry.specifier.clone(), version.clone());
        }
    }
    let mut resolved: BTreeMap<String, String> = BTreeMap::new();
    for (name, specifier) in dependencies {
        match locked.get(specifier) {
            Some(version) => {
                resolved.insert(name.clone(), version.clone());
            }
            None => report.skipped.push(Skipped {
                name: name.clone(),
                range: specifier.clone(),
                reason: SkippedReason::UnknownPackage,
            }),
        }
    }
    Ok((resolved, RegistryIndex::default()))
}

fn manifest_path(manifests_dir: &Path, name: &str, version: &str) -> PathBuf {
    let safe_name = name.replace('/', "_");
    manifests_dir.join(format!("{safe_name}@{version}.json"))
}

#[cfg(feature = "registry")]
fn save_manifest(
    manifests_dir: &Path,
    name: &str,
    version: &str,
    manifest: &PackageManifest,
) -> Result<(), InstallError> {
    fs::write(
        manifest_path(manifests_dir, name, version),
        manifest.to_json(),
    )?;
    Ok(())
}

fn load_manifest(
    manifests_dir: &Path,
    name: &str,
    version: &str,
) -> Result<PackageManifest, InstallError> {
    let path = manifest_path(manifests_dir, name, version);
    let raw = fs::read_to_string(&path).map_err(|_| InstallError::MissingManifest {
        name: name.to_string(),
        version: version.to_string(),
    })?;
    Ok(PackageManifest::from_json(&raw)?)
}

fn read_package_json(real_dir: &Path) -> Result<PackageJson, InstallError> {
    let raw = fs::read_to_string(real_dir.join("package.json"))?;
    Ok(PackageJson::parse(&raw)?)
}

fn build_lockfile(packages: &BTreeMap<String, String>, index: &RegistryIndex) -> Lockfile {
    let mut lockfile = Lockfile {
        lockfile_version: LOCKFILE_VERSION.to_string(),
        ..Lockfile::default()
    };
    let mut records: BTreeMap<String, PackageRecord> = BTreeMap::new();
    let mut snapshots: BTreeMap<String, SnapshotRecord> = BTreeMap::new();
    for (name, version) in packages {
        let key = format!("{name}@{version}");
        let integrity = index
            .entry(name, version)
            .and_then(|entry| entry.integrity.clone())
            .unwrap_or_default();
        records.insert(
            key.clone(),
            PackageRecord {
                resolution: ResolutionInfo { integrity },
            },
        );
        let mut snapshot = SnapshotRecord::default();
        if let Some(entry) = index.entry(name, version) {
            for dep_name in entry.dependencies.keys() {
                if let Some(dep_version) = packages.get(dep_name) {
                    snapshot
                        .dependencies
                        .insert(dep_name.clone(), dep_version.clone());
                }
            }
        }
        snapshots.insert(key, snapshot);
    }
    lockfile.packages = records;
    lockfile.snapshots = snapshots;
    lockfile
}

fn importer_entry(
    dependencies: &[(String, String)],
    resolved: &BTreeMap<String, String>,
) -> ImporterEntry {
    let mut entry = ImporterEntry::default();
    for (name, specifier) in dependencies {
        let version = resolved.get(name).cloned();
        entry.dependencies.insert(
            name.clone(),
            DependencyEntry::new(specifier.clone(), version),
        );
    }
    entry
}

fn sorted_unique(dependencies: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for (name, range) in dependencies {
        if seen.insert(name.clone()) {
            unique.push((name, range));
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "registry")]
    struct MockRegistry {
        listener: std::net::TcpListener,
    }

    #[cfg(feature = "registry")]
    impl MockRegistry {
        fn new() -> Self {
            Self {
                listener: std::net::TcpListener::bind("127.0.0.1:0").unwrap(),
            }
        }

        fn url(&self) -> String {
            format!("http://{}", self.listener.local_addr().unwrap())
        }

        fn serve(
            self,
            packuments: Vec<(String, serde_json::Value)>,
            tarballs: Vec<(String, Vec<u8>)>,
        ) {
            std::thread::spawn(move || {
                let listener = self.listener;
                'outer: loop {
                    let Ok((mut stream, _)) = listener.accept() else {
                        return;
                    };
                    let mut buffer = Vec::new();
                    let mut byte = [0u8; 1];
                    loop {
                        match stream.read(&mut byte) {
                            Ok(0) => break,
                            Ok(_) => {
                                buffer.push(byte[0]);
                                if buffer.ends_with(b"\r\n\r\n") {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    let request = String::from_utf8_lossy(&buffer).to_string();
                    let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                    let mut response: Option<(u16, String, Vec<u8>)> = None;
                    for (name, packument) in &packuments {
                        let encoded = name.replace('/', "%2F");
                        if path == format!("/{encoded}") || path == format!("/{name}") {
                            response = Some((
                                200,
                                String::from("application/json"),
                                serde_json::to_vec(packument).unwrap(),
                            ));
                            break;
                        }
                    }
                    if response.is_none() {
                        for (suffix, body) in &tarballs {
                            if path == format!("/{suffix}") {
                                response =
                                    Some((200, String::from("application/x-tgz"), body.clone()));
                                break;
                            }
                        }
                    }
                    let Some((status, content_type, body)) = response else {
                        let _ = stream.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        continue 'outer;
                    };
                    let head = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&body);
                }
            });
        }
    }

    #[cfg(feature = "registry")]
    use std::io::{Read as _, Write as _};

    #[cfg(feature = "registry")]
    fn build_tgz(files: &[(&str, &str)]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use tar::Builder;
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("package/{path}"), content.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn temp_project(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pnpm-guru-install-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[cfg(feature = "registry")]
    #[test]
    fn installs_two_packages_from_mock_registry() {
        let registry = MockRegistry::new();
        let base = registry.url();
        let tgz_a = build_tgz(&[
            (
                "package.json",
                r#"{"name":"alpha","version":"1.0.0","dependencies":{"beta":"^2.0.0"},"bin":{"alpha":"run.js"}}"#,
            ),
            ("run.js", "#!/usr/bin/env node\nconsole.log('a')\n"),
            ("index.js", "module.exports = 1\n"),
        ]);
        let tgz_b = build_tgz(&[
            ("package.json", r#"{"name":"beta","version":"2.1.0"}"#),
            ("index.js", "module.exports = 2\n"),
        ]);
        let integrity_a = crate::tarball::ssri_sha512(&tgz_a);
        let integrity_b = crate::tarball::ssri_sha512(&tgz_b);
        let packuments = vec![
            (
                String::from("alpha"),
                serde_json::json!({"versions": {"1.0.0": {"dependencies": {"beta": "^2.0.0"}, "dist": {
                    "integrity": integrity_a,
                    "tarball": format!("{base}/alpha-1.0.0.tgz")
                }}}}),
            ),
            (
                String::from("beta"),
                serde_json::json!({"versions": {"2.1.0": {"dist": {
                    "integrity": integrity_b,
                    "tarball": format!("{base}/beta-2.1.0.tgz")
                }}}}),
            ),
        ];
        registry.serve(
            packuments,
            vec![
                (String::from("alpha-1.0.0.tgz"), tgz_a),
                (String::from("beta-2.1.0.tgz"), tgz_b),
            ],
        );

        let project = temp_project("mock");
        write(
            &project.join("package.json"),
            r#"{"name":"app","version":"0.1.0","dependencies":{"alpha":"^1.0.0"}}"#,
        );

        let options = InstallOptions {
            registry: base,
            ..InstallOptions::default()
        };
        let report = install(&project, &options).unwrap();

        assert_eq!(report.installed.len(), 2);
        let alpha = report.installed.iter().find(|p| p.name == "alpha").unwrap();
        assert_eq!(alpha.version, "1.0.0");
        assert_eq!(alpha.bins, vec!["alpha".to_string()]);
        let beta = report.installed.iter().find(|p| p.name == "beta").unwrap();
        assert_eq!(beta.version, "2.1.0");

        let alias = project.join("node_modules").join("alpha");
        let entry = fs::read_to_string(alias.join("run.js")).unwrap();
        assert!(entry.contains("console.log('a')"));
        let real = project
            .join("node_modules")
            .join(".pnpm")
            .join("alpha@1.0.0")
            .join("node_modules")
            .join("alpha")
            .join("index.js");
        assert!(real.is_file());
        assert!(project.join("node_modules").join(".bin").exists());

        let lockfile = fs::read_to_string(project.join("pnpm-lock.yaml")).unwrap();
        assert!(lockfile.contains("lockfileVersion: '9.0'"));
        assert!(lockfile.contains("alpha@1.0.0"));
        assert!(lockfile.contains("beta@2.1.0"));

        let _ = fs::remove_dir_all(&project);
    }

    #[cfg(feature = "registry")]
    #[test]
    fn offline_reinstall_uses_store_and_manifests() {
        let registry = MockRegistry::new();
        let base = registry.url();
        let tgz = build_tgz(&[
            ("package.json", r#"{"name":"solo","version":"3.0.0"}"#),
            ("index.js", "module.exports = 3\n"),
        ]);
        let integrity = crate::tarball::ssri_sha512(&tgz);
        let packuments = vec![(
            String::from("solo"),
            serde_json::json!({"versions": {"3.0.0": {"dist": {
                "integrity": integrity,
                "tarball": format!("{base}/solo-3.0.0.tgz")
            }}}}),
        )];
        registry.serve(packuments, vec![(String::from("solo-3.0.0.tgz"), tgz)]);
        let project = temp_project("offline");
        write(
            &project.join("package.json"),
            r#"{"name":"app","dependencies":{"solo":"^3.0.0"}}"#,
        );
        let online = InstallOptions {
            registry: base.clone(),
            ..InstallOptions::default()
        };
        install(&project, &online).unwrap();

        let offline = InstallOptions {
            registry: base,
            offline: true,
            ..InstallOptions::default()
        };
        let report = install(&project, &offline).unwrap();
        assert_eq!(report.installed.len(), 1);
        assert_eq!(report.installed[0].name, "solo");
        let real = project
            .join("node_modules")
            .join(".pnpm")
            .join("solo@3.0.0")
            .join("node_modules")
            .join("solo")
            .join("index.js");
        assert!(real.is_file());
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn offline_without_lockfile_fails() {
        let project = temp_project("nolock");
        write(
            &project.join("package.json"),
            r#"{"name":"app","dependencies":{"solo":"^3.0.0"}}"#,
        );
        let options = InstallOptions {
            offline: true,
            ..InstallOptions::default()
        };
        let error = install(&project, &options).unwrap_err();
        assert!(matches!(error, InstallError::OfflineNeedsLockfile));
        let _ = fs::remove_dir_all(&project);
    }
}
