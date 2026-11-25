use crate::package::PackageJson;
use std::fs;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum BinError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn bin_entries(package_json: &PackageJson) -> Vec<(String, String)> {
    match &package_json.bin {
        Some(field) => {
            let name = package_json
                .name
                .clone()
                .unwrap_or_else(|| String::from("<package>"));
            field.entries(&name)
        }
        None => Vec::new(),
    }
}

fn escape_cmd(text: &str) -> String {
    text.replace('"', "\"\"")
}

fn cmd_shim(target_relative: &str) -> String {
    let target = target_relative.replace('/', "\\");
    format!(
        "@ECHO off\r\nSETLOCAL\r\nSET \"_target=%~dp0{target}\"\r\nnode \"%_target%\" %*\r\n",
        target = escape_cmd(&target)
    )
}

fn ps1_shim(target_relative: &str) -> String {
    let target = target_relative.replace('/', "\\");
    format!(
        "#!/usr/bin/env pwsh\r\n$target = Join-Path $PSScriptRoot '{target}'\r\n& node $target @args\r\nexit $LASTEXITCODE\r\n",
        target = target.replace('\'', "''")
    )
}

fn sh_shim(target_relative: &str) -> String {
    format!(
        "#!/bin/sh\nbasedir=$(dirname \"$0\")\nexec node \"$basedir/{target}\" \"$@\"\n",
        target = target_relative
    )
}

pub fn link_bins(
    node_modules: &Path,
    package_json: &PackageJson,
    real_dir: &Path,
) -> Result<Vec<String>, BinError> {
    let Some(alias) = package_json.name.as_deref() else {
        return Ok(Vec::new());
    };
    let mut linked = Vec::new();
    let entries = bin_entries(package_json);
    if entries.is_empty() {
        return Ok(linked);
    }
    let bin_dir = node_modules.join(".bin");
    fs::create_dir_all(&bin_dir)?;
    for (bin_name, bin_path) in &entries {
        let target = real_dir.join(bin_path);
        if !target.is_file() {
            continue;
        }
        let relative = format!(
            "../{}/{bin_path}",
            crate::node_modules::alias_dir_name(alias)
        );
        #[cfg(unix)]
        {
            let link = bin_dir.join(bin_name);
            let _ = fs::remove_file(&link);
            std::os::unix::fs::symlink(&relative, &link)?;
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target)?.permissions().mode();
            let _ = fs::set_permissions(&link, fs::Permissions::from_mode(mode | 0o111));
        }
        #[cfg(windows)]
        {
            let _ = fs::write(bin_dir.join(format!("{bin_name}.cmd")), cmd_shim(&relative));
            let _ = fs::write(bin_dir.join(format!("{bin_name}.ps1")), ps1_shim(&relative));
            let _ = fs::write(bin_dir.join(bin_name), sh_shim(&relative));
        }
        linked.push(bin_name.clone());
    }
    Ok(linked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pnpm-guru-bins-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn package(raw: &str) -> PackageJson {
        PackageJson::parse(raw).unwrap()
    }

    #[test]
    fn bin_entries_single_and_map() {
        let single = package(r#"{"name":"tools","bin":"cli.js"}"#);
        assert_eq!(
            bin_entries(&single),
            vec![("tools".to_string(), "cli.js".to_string())]
        );
        let map = package(r#"{"name":"multi","bin":{"one":"a.js","two":"b.js"}}"#);
        let entries = bin_entries(&map);
        assert_eq!(
            entries,
            vec![
                ("one".to_string(), "a.js".to_string()),
                ("two".to_string(), "b.js".to_string())
            ]
        );
    }

    #[test]
    fn package_without_bin_links_nothing() {
        let root = temp_root("nobin");
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let package_json = package(r#"{"name":"plain","version":"1.0.0"}"#);
        let node_modules = root.join("node_modules");
        let linked = link_bins(&node_modules, &package_json, &real).unwrap();
        assert!(linked.is_empty());
        assert!(!node_modules.join(".bin").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_bin_file_is_skipped() {
        let root = temp_root("missing");
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let package_json = package(r#"{"name":"ghost","bin":{"ghost":"nowhere.js"}}"#);
        let node_modules = root.join("node_modules");
        let linked = link_bins(&node_modules, &package_json, &real).unwrap();
        assert!(linked.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn unix_symlink_points_to_alias() {
        let root = temp_root("unix");
        let real = root
            .join(".pnpm")
            .join("tools@1.0.0")
            .join("node_modules")
            .join("tools");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
        let package_json = package(r#"{"name":"tools","bin":{"tools":"cli.js"}}"#);
        let node_modules = root.join("node_modules");
        fs::create_dir_all(&node_modules).unwrap();
        let alias = node_modules.join("tools");
        crate::node_modules::create_alias(&real, &alias).unwrap();
        let linked = link_bins(&node_modules, &package_json, &real).unwrap();
        assert_eq!(linked, vec!["tools".to_string()]);
        let link = node_modules.join(".bin").join("tools");
        let target = fs::read_link(&link).unwrap();
        assert_eq!(target.to_string_lossy(), "../tools/cli.js");
        let resolved = fs::canonicalize(&link).unwrap();
        assert_eq!(resolved, fs::canonicalize(&real.join("cli.js")).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_shims_reference_alias() {
        let root = temp_root("win");
        let real = root
            .join(".pnpm")
            .join("tools@1.0.0")
            .join("node_modules")
            .join("tools");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("cli.js"), "// entry").unwrap();
        let package_json = package(r#"{"name":"tools","bin":{"tools":"cli.js"}}"#);
        let node_modules = root.join("node_modules");
        fs::create_dir_all(&node_modules).unwrap();
        let alias = node_modules.join("tools");
        crate::node_modules::create_alias(&real, &alias).unwrap();
        let linked = link_bins(&node_modules, &package_json, &real).unwrap();
        assert_eq!(linked, vec!["tools".to_string()]);
        let cmd = fs::read_to_string(node_modules.join(".bin").join("tools.cmd")).unwrap();
        assert!(cmd.starts_with("@ECHO off"));
        assert!(cmd.contains("%~dp0..\\tools\\cli.js"));
        assert!(cmd.contains("node \"%_target%\" %*"));
        let ps1 = fs::read_to_string(node_modules.join(".bin").join("tools.ps1")).unwrap();
        assert!(ps1.contains("..\\tools\\cli.js"));
        let sh = fs::read_to_string(node_modules.join(".bin").join("tools")).unwrap();
        assert!(sh.contains("../tools/cli.js"));
        let _ = fs::remove_dir_all(&root);
    }
}
