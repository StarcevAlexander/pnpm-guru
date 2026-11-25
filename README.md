# pnpm-guru

**pnpm-style package manager engine for Rust** — package.json parsing, npm semver ranges, dependency resolution, a registry HTTP client, tarball installs with integrity checks, lifecycle scripts, bin linking, pnpm-lock.yaml read/write, a content-addressable store and pnpm's symlinked node_modules layout.

The engine follows pnpm's architecture as a library: embed it in your build tool, run fully offline installs from a pre-filled store, and keep CI installs deterministic. It is not affiliated with the pnpm team and does not aim to replace the pnpm CLI.

## Install

```toml
[dependencies]
pnpm-guru = "0.4"
```

The `registry` feature (on by default) brings the HTTP client (`ureq` with native-tls). Disable default features for a network-free build; `flate2` and `tar` stay pure Rust.

## One-shot install

```rust
use pnpm_guru::{InstallOptions, install};

let options = InstallOptions {
    registry: String::from("https://registry.npmjs.org"),
    production: false,
    ignore_scripts: false,
    offline: false,
};

let report = install(std::path::Path::new("."), &options).unwrap();
for package in &report.installed {
    println!("+ {}@{} ({} files, bins: {:?})",
        package.name, package.version, package.files, package.bins);
}
println!("skipped: {:?}", report.skipped);
```

The installer reads the project's package.json, resolves the dependency graph against packuments, downloads and integrity-checks each tarball, unpacks it into the content-addressable store, assembles pnpm's `.pnpm/<name>@<version>/node_modules/<name>` layout with top-level aliases, links package bins into `node_modules/.bin`, runs `preinstall`/`install`/`postinstall` scripts and writes pnpm-lock.yaml v9.

`workspace:` and `file:`/`link:` dependencies are reported in `report.skipped` instead of being installed.

## Offline installs

After one online install the store, stored manifests and the lockfile are complete, and the same project installs again with no network:

```rust
let options = pnpm_guru::InstallOptions {
    offline: true,
    ..Default::default()
};
let report = pnpm_guru::install(path, &options).unwrap();
```

Offline mode reassembles node_modules from the store and the versions recorded in pnpm-lock.yaml, and skips lifecycle scripts — deterministic by construction.

## Registry client

```rust
use pnpm_guru::{DEFAULT_REGISTRY, RegistryClient};

let client = RegistryClient::new(DEFAULT_REGISTRY);
let index = client.packument_index("left-pad")?;
let entry = index.entry("left-pad", "1.3.0").unwrap();
println!("integrity: {:?}", entry.integrity);
let tarball = client.tarball(entry.tarball.as_deref().unwrap())?;
```

`packument_index` reads the abbreviated install metadata (`Accept: application/vnd.npm.install-v1+json`), understands scoped names, and fills each entry with dependencies plus `dist.integrity` and `dist.tarball`. Transitive dependencies are fetched as the resolver discovers them.

## Tarballs and integrity

```rust
use pnpm_guru::{ContentStore, ssri_sha512, unpack_tarball};

let store = ContentStore::new(".pnpm-guru-store");
let unpacked = unpack_tarball(&store, &tgz_bytes, Some("sha512-..."), "left-pad", "1.3.0")?;
assert_eq!(unpacked.manifest.files.len(), unpacked.files);
```

Tarballs are verified against their SSRI integrity (`sha512-`, `sha256-`), unpacked into the store, and summarized as a `PackageManifest` of `path → {hash, size}` records ready for `assemble`.

## Lifecycle scripts

```rust
use pnpm_guru::{lifecycle_scripts, package_env, run_script};

for (script, line) in lifecycle_scripts(&package.scripts) {
    run_script(&name, &script, &line, &pkg_dir, &package_env(&name, &version, &pkg_dir, &node_modules))?;
}
```

Scripts run through `sh -c` (unix) or `cmd /C` (windows) with the package directory as cwd and `npm_package_name`, `npm_package_version`, `npm_package_json` and `INIT_CWD` in the environment.

## Bin linking

Packages exposing `bin` entries get them linked into `node_modules/.bin` — symlinks on unix, `cmd`/`ps1`/`sh` shims on windows, all resolving through the package's top-level alias.

## Semver ranges

`parse_range` / `satisfies` speak npm dialect: `^1.2.3`, `~2.1.0`, `1.x`, `>=2.0.0 <3.0.0`, `2.3.0 - 2.9.9`, `^2.0.0 || ^3.0.0`.

## Resolution model

Like pnpm's flat plan: each package name resolves once to the highest version satisfying the FIRST range that requested it; later ranges wanting a different version are recorded as `conflicts`. Unresolvable requests are reported in `skipped` with reasons (unknown package, no matching version, invalid range, `workspace:`/`file:`/`link:` protocols) instead of failing the whole run.

## Workspaces

`workspace:` protocol dependencies resolve against packages discovered from the root `workspaces` globs and are reported in `resolution.workspace_links` — symlinks, not registry installs.

## Lockfile

`parse_lockfile` / `write_lockfile` read and write pnpm-lock.yaml v9 — importers, packages (integrity) and snapshots, all `BTreeMap`-ordered for stable diffs. The installer writes importers, per-package integrity records and dependency snapshots.

## Roadmap

- ~~0.2: content-addressable store + node_modules assembly~~ (done)
- ~~0.3: pnpm-lock.yaml read/write, workspaces protocol resolution~~ (done)
- ~~0.4: registry client, tarball installs, lifecycle scripts, bin linking, CLI~~ (done)

## CLI

A minimal `pnpm-guru` binary ships with the crate:

```
pnpm-guru install [--production] [--offline] [--ignore-scripts] [--registry <url>] [--dir <path>]
```

## Testing

The test suite installs real packages against a mock registry served on a local socket, verifies sha512 integrity end to end, replays offline installs from the store, and — behind the `PG_NET=1` flag — talks to the live npm registry.

## License

Licensed under either of MIT or Apache-2.0, at your option.

Independent implementation inspired by pnpm's architecture; not affiliated with the pnpm team. Semver range logic mirrors npm semantics via the node-semver crate.
