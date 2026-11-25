use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("script {script:?} of {package} failed with {status}")]
    Failed {
        package: String,
        script: String,
        status: String,
    },
    #[error("failed to spawn shell: {0}")]
    Spawn(#[from] std::io::Error),
}

pub const DEPENDENCY_LIFECYCLE_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

pub fn shell_command(script: &str, cwd: &Path) -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script).current_dir(cwd);
        command
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(script).current_dir(cwd);
        command
    }
}

pub fn run_script(
    package_name: &str,
    script: &str,
    command_line: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> Result<(), ScriptError> {
    let mut command = shell_command(command_line, cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    let status = command.status()?;
    if status.success() {
        return Ok(());
    }
    Err(ScriptError::Failed {
        package: package_name.to_string(),
        script: script.to_string(),
        status: status.to_string(),
    })
}

pub fn lifecycle_scripts(scripts: &HashMap<String, String>) -> Vec<(String, String)> {
    DEPENDENCY_LIFECYCLE_SCRIPTS
        .iter()
        .filter_map(|name| {
            scripts
                .get(*name)
                .map(|line| ((*name).to_string(), line.clone()))
        })
        .collect()
}

pub fn package_env(
    package_name: &str,
    version: &str,
    package_dir: &Path,
    node_modules: &Path,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert(String::from("npm_package_name"), package_name.to_string());
    env.insert(String::from("npm_package_version"), version.to_string());
    env.insert(
        String::from("npm_package_json"),
        package_dir
            .join("package.json")
            .to_string_lossy()
            .to_string(),
    );
    env.insert(
        String::from("INIT_CWD"),
        package_dir.to_string_lossy().to_string(),
    );
    env.insert(
        String::from("npm_config_local_prefix"),
        node_modules.parent().map_or_else(
            || node_modules.to_string_lossy().to_string(),
            |p| p.to_string_lossy().to_string(),
        ),
    );
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pnpm-guru-lifecycle-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn runs_shell_script_and_creates_marker() {
        let dir = temp_dir("ok");
        let marker = dir.join("marker.txt");
        let script = if cfg!(windows) {
            "echo done> marker.txt"
        } else {
            "echo done > marker.txt"
        };
        run_script("demo", "postinstall", script, &dir, &HashMap::new()).unwrap();
        let content = std::fs::read_to_string(&marker).unwrap();
        assert!(content.contains("done"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failing_script_reports() {
        let dir = temp_dir("fail");
        let error = run_script("demo", "install", "exit 3", &dir, &HashMap::new()).unwrap_err();
        match error {
            ScriptError::Failed {
                package, script, ..
            } => {
                assert_eq!(package, "demo");
                assert_eq!(script, "install");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lifecycle_scripts_are_filtered_in_order() {
        let mut scripts = HashMap::new();
        scripts.insert("postinstall".to_string(), String::from("c"));
        scripts.insert("preinstall".to_string(), String::from("a"));
        scripts.insert("build".to_string(), String::from("skip"));
        scripts.insert("test".to_string(), String::from("skip"));
        let picked = lifecycle_scripts(&scripts);
        assert_eq!(
            picked,
            vec![
                (String::from("preinstall"), String::from("a")),
                (String::from("postinstall"), String::from("c"))
            ]
        );
    }

    #[test]
    fn env_exposes_package_identity() {
        let dir = temp_dir("env");
        let node_modules = dir.join("node_modules");
        std::fs::create_dir_all(&node_modules).unwrap();
        let env = package_env("left-pad", "1.3.0", &dir, &node_modules);
        assert_eq!(env["npm_package_name"], "left-pad");
        assert_eq!(env["npm_package_version"], "1.3.0");
        assert!(env["npm_package_json"].contains("package.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
