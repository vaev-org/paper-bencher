use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use anyhow::{Context, Result, bail};

pub fn resolve_program(program: &Path) -> Result<PathBuf> {
    if program.components().count() > 1 {
        return fs::canonicalize(program)
            .with_context(|| format!("cannot resolve executable {}", program.display()));
    }

    let path = env::var_os("PATH").context("PATH is not set")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(program);
        if candidate.is_file() {
            return fs::canonicalize(&candidate)
                .with_context(|| format!("cannot resolve executable {}", candidate.display()));
        }
    }
    bail!(
        "required executable `{}` was not found in PATH",
        program.display()
    )
}

pub fn require_program(name: &str) -> Result<PathBuf> {
    resolve_program(Path::new(name))
}

pub fn checked_output(command: &mut Command) -> Result<Output> {
    let display = display_command(command);
    let output = command
        .output()
        .with_context(|| format!("failed to start `{display}`"))?;
    if !output.status.success() {
        bail!(
            "`{display}` exited with {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

pub fn checked_text(command: &mut Command) -> Result<String> {
    let output = checked_output(command)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn run_logged(command: &mut Command, log_path: &Path) -> Result<()> {
    let display = display_command(command);
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to start `{display}`"))?;

    let mut log = fs::File::create(log_path)
        .with_context(|| format!("cannot create {}", log_path.display()))?;
    log.write_all(&output.stdout)?;
    log.write_all(&output.stderr)?;

    if !output.status.success() {
        bail!(
            "`{display}` exited with {}; see {}",
            output.status,
            log_path.display()
        );
    }
    Ok(())
}

pub fn capture_to_file(command: &mut Command, output_path: &Path) -> Result<String> {
    let display = display_command(command);
    let output = command
        .output()
        .with_context(|| format!("failed to start `{display}`"))?;

    let mut combined = output.stdout;
    combined.extend_from_slice(&output.stderr);
    fs::write(output_path, &combined)
        .with_context(|| format!("cannot write {}", output_path.display()))?;

    if !output.status.success() {
        bail!(
            "`{display}` exited with {}; see {}",
            output.status,
            output_path.display()
        );
    }
    Ok(String::from_utf8_lossy(&combined).into_owned())
}

pub fn display_command(command: &Command) -> String {
    let mut parts = vec![quote_os(command.get_program())];
    parts.extend(command.get_args().map(quote_os));
    parts.join(" ")
}

pub fn direct_command_string(program: &Path, args: &[OsString]) -> String {
    let mut parts = vec![quote_os(program.as_os_str())];
    parts.extend(args.iter().map(|arg| quote_os(arg.as_os_str())));
    parts.join(" ")
}

fn quote_os(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._:=+-".contains(c))
    {
        value.into_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\\\''"))
    }
}
