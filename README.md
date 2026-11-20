# pnpm-guru

**pnpm-style package manager core for Rust** — package.json parsing, npm semver ranges and dependency resolution over an offline registry index.

Part 1 of the guru family's package-manager project: the pure-Rust core (parsing, range matching, resolution) that later phases build on — content-addressable store, symlinked node_modules, lockfiles, workspaces, lifecycle scripts and a CLI are phased in next (see roadmap).

## Usage

```rust
use pnpm_guru::{parse_range, resolve, PackageJson, RegistryIndex};
use serde_json::json;

fn main() {
    let package = PackageJson::parse(r#"{"dependencies": {"express": "^4.18.0"}}"#).unwrap();

    let registry = json!({
        "express": {
            "4.17.1": {"dependencies": {"accepts": "~1.3.7"}},
            "4.18.2": {"dependencies": {"accepts": "~1.3.8"}}
        },
        "accepts": {"1.3.8": {}}
    });
    let index = RegistryIndex::from_json(&registry);

    let resolution = resolve(&package.production_dependencies(), &index);
    assert_eq!(resolution.packages["express"], "4.18.2");
    assert_eq!(resolution.packages["accepts"], "1.3.8");
}
```

## Semver ranges

`parse_range` / `satisfies` speak npm dialect: `^1.2.3`, `~2.1.0`, `1.x`, `>=2.0.0 <3.0.0`, `2.3.0 - 2.9.9`, `^2.0.0 || ^3.0.0`.

## Resolution model

Like pnpm's flat plan: each package name resolves once to the highest version satisfying the FIRST range that requested it; later ranges wanting a different version are recorded as `conflicts`. Unresolvable requests are reported in `skipped` with reasons (`workspace:` protocol, unknown package, no matching version, invalid range) instead of failing the whole run.

## Workspaces

`workspace:` protocol dependencies resolve against packages discovered from the root `workspaces` globs and are reported in `resolution.workspace_links` — symlinks, not registry installs:

```rust
use pnpm_guru::{resolve_with_workspaces, workspaces};
use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let workspaces = workspaces::discover(Path::new("."), &["packages/*".to_string()]);
    let versions: BTreeMap<_, _> = workspaces
        .iter()
        .map(|p| (p.name.clone(), p.version.clone()))
        .collect();
    let resolution = resolve_with_workspaces(&root_deps, &index, &versions);
    println!("{:?}", resolution.workspace_links);
}
```

## Lockfile

`lockfile::parse_lockfile` / `lockfile::write_lockfile` read and write pnpm-lock.yaml v9 — importers, packages (integrity) and snapshots, all `BTreeMap`-ordered for stable diffs.

## Roadmap

- ~~0.2: content-addressable store + node_modules assembly~~ (done)
- ~~0.3: pnpm-lock.yaml read/write, workspaces protocol resolution~~ (done)
- 0.4: lifecycle scripts, registry HTTP client

## License

Licensed under either of MIT or Apache-2.0, at your option.

Independent implementation inspired by pnpm's architecture; not affiliated with the pnpm team. Semver range logic mirrors npm semantics via the node-semver crate.
