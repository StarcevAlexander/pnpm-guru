use std::path::PathBuf;
use std::process::ExitCode;

use pnpm_guru::{InstallOptions, install};

fn usage() -> String {
    String::from(
        "pnpm-guru - pnpm-style package manager core\n\n\
         USAGE:\n    pnpm-guru install [OPTIONS]\n    pnpm-guru --version\n\n\
         OPTIONS:\n    --production        skip devDependencies\n    --offline           install from the store and pnpm-lock.yaml only\n    --ignore-scripts    do not run lifecycle scripts\n    --registry <url>    npm registry to fetch from\n    --dir <path>        project directory (defaults to the current one)\n    -h, --help          show this help",
    )
}

struct Cli {
    project: PathBuf,
    options: InstallOptions,
}

fn parse_args(args: &[String]) -> Result<Option<Cli>, String> {
    let mut command: Option<&str> = None;
    let mut cli = Cli {
        project: PathBuf::from("."),
        options: InstallOptions::default(),
    };
    let mut iterator = args.iter().peekable();
    while let Some(arg) = iterator.next() {
        match arg.as_str() {
            "--version" => {
                println!("pnpm-guru {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "-h" | "--help" => {
                println!("{}", usage());
                return Ok(None);
            }
            "--production" => cli.options.production = true,
            "--offline" => cli.options.offline = true,
            "--ignore-scripts" => cli.options.ignore_scripts = true,
            "--registry" => {
                let value = iterator.next().ok_or("--registry needs a value")?;
                cli.options.registry = value.clone();
            }
            "--dir" => {
                let value = iterator.next().ok_or("--dir needs a value")?;
                cli.project = PathBuf::from(value);
            }
            other => {
                if command.is_none() && !other.starts_with('-') {
                    command = Some(other);
                } else {
                    return Err(format!("unknown argument {other:?}\n\n{}", usage()));
                }
            }
        }
    }
    match command {
        None => Err(format!("expected a command\n\n{}", usage())),
        Some("install") => Ok(Some(cli)),
        Some(other) => Err(format!(
            "unknown command {other:?}, only `install` is supported\n\n{}",
            usage()
        )),
    }
}

fn real_main() -> Result<ExitCode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cli) = parse_args(&args)? else {
        return Ok(ExitCode::SUCCESS);
    };
    let report = install(&cli.project, &cli.options).map_err(|error| error.to_string())?;
    for package in &report.installed {
        let bins = if package.bins.is_empty() {
            String::new()
        } else {
            format!(" bins: {}", package.bins.join(", "))
        };
        println!(
            "+ {}@{} ({} files{})",
            package.name, package.version, package.files, bins
        );
    }
    for skipped in &report.skipped {
        println!(
            "- {} {} ({:?})",
            skipped.name, skipped.range, skipped.reason
        );
    }
    for conflict in &report.conflicts {
        println!(
            "! {} kept {} rejected {} (requested {})",
            conflict.name,
            conflict.kept_version,
            conflict.rejected_version,
            conflict.requested_range
        );
    }
    println!(
        "{} packages installed, lockfile {}",
        report.installed.len(),
        if report.lockfile_written {
            "written"
        } else {
            "not written"
        }
    );
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
