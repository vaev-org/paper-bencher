use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
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
    let spinner = Spinner::start(progress_name(command));
    let output_result = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    spinner.finish(
        output_result
            .as_ref()
            .is_ok_and(|output| output.status.success()),
    );
    let output = output_result.with_context(|| format!("failed to start `{display}`"))?;

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
    let spinner = Spinner::start(progress_name(command));
    let output_result = command.output();
    spinner.finish(
        output_result
            .as_ref()
            .is_ok_and(|output| output.status.success()),
    );
    let output = output_result.with_context(|| format!("failed to start `{display}`"))?;

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

fn progress_name(command: &Command) -> String {
    let program = Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_string_lossy();
    let action = command
        .get_args()
        .next()
        .and_then(|argument| argument.to_str())
        .filter(|argument| !argument.starts_with('-'));
    match action {
        Some(action) => format!("{program} {action}"),
        None => program.into_owned(),
    }
}

struct Spinner {
    active: bool,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    label: String,
}

impl Spinner {
    fn start(label: String) -> Self {
        let active = std::io::stderr().is_terminal();
        let running = Arc::new(AtomicBool::new(active));
        let handle = active.then(|| {
            let running = Arc::clone(&running);
            let label = label.clone();
            thread::spawn(move || {
                let frames = ["◐", "◓", "◑", "◒"];
                let mut index = 0;
                while running.load(Ordering::Relaxed) {
                    eprint!("\r{} {}", frames[index % frames.len()], label);
                    let _ = std::io::stderr().flush();
                    index += 1;
                    thread::sleep(Duration::from_millis(120));
                }
            })
        });
        Self {
            active,
            running,
            handle,
            label,
        }
    }

    fn finish(mut self, success: bool) {
        if !self.active {
            return;
        }
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let mark = if success { "✓" } else { "✗" };
        eprintln!("\r\x1b[2K{mark} {}", self.label);
    }
}
