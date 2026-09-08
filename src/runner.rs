use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};

use crate::{
    cli::{BenchTool, RunArgs},
    model::Metadata,
    process::{
        capture_to_file, checked_text, direct_command_string, require_program, resolve_program,
        run_logged,
    },
};

const GENERATOR: &str = include_str!("../assets/general-ledger.py");
const STYLESHEET: &str = include_str!("../assets/general-ledger.css");

pub fn run(args: RunArgs) -> Result<()> {
    let repo = git_root()?;
    run_at(args, &repo, &repo)
}

pub(crate) fn run_at(args: RunArgs, repo: &Path, artifact_repo: &Path) -> Result<()> {
    validate_label(&args.label)?;
    ensure!(
        args.ledger_size.is_multiple_of(100),
        "--ledger-size must be a multiple of 100"
    );
    let bench_root = artifact_repo.join(".bench");
    let target = bench_root.join(&args.label);
    let selected = selected_tools(&args)?;

    if target.exists() && !args.force {
        bail!(
            "benchmark run `{}` already exists at {}; pass --force to replace it",
            args.label,
            target.display()
        );
    }

    let external_binary = args.binary.as_deref().map(resolve_program).transpose()?;
    let python = require_program("python3")?;
    let hyperfine = optional_program(&selected, BenchTool::Hyperfine, "hyperfine")?;
    let perf = optional_program(&selected, BenchTool::Perf, "perf")?;
    let heaptrack = optional_program(&selected, BenchTool::Heaptrack, "heaptrack")?;
    let heaptrack_print = optional_program(&selected, BenchTool::Heaptrack, "heaptrack_print")?;
    let git = require_program("git")?;

    fs::create_dir_all(&bench_root)
        .with_context(|| format!("cannot create {}", bench_root.display()))?;
    let stage = bench_root.join(format!(".{}-{}-staging", args.label, std::process::id()));
    if stage.exists() {
        fs::remove_dir_all(&stage)
            .with_context(|| format!("cannot clear stale staging directory {}", stage.display()))?;
    }
    fs::create_dir_all(stage.join("templates"))?;

    let result = collect(
        &args,
        repo,
        &stage,
        &target,
        &selected,
        external_binary.as_deref(),
        &python,
        hyperfine.as_deref(),
        perf.as_deref(),
        heaptrack.as_deref(),
        heaptrack_print.as_deref(),
        &git,
    );

    if let Err(error) = result {
        return Err(error).with_context(|| {
            format!(
                "run did not complete; partial artifacts were left in {}",
                stage.display()
            )
        });
    }

    publish(&stage, &target, args.force)?;
    println!("completed `{}`: {}", args.label, target.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect(
    args: &RunArgs,
    repo: &Path,
    stage: &Path,
    target: &Path,
    selected: &BTreeSet<BenchTool>,
    external_binary: Option<&Path>,
    python: &Path,
    hyperfine: Option<&Path>,
    perf: Option<&Path>,
    heaptrack: Option<&Path>,
    heaptrack_print: Option<&Path>,
    git: &Path,
) -> Result<()> {
    let generator = stage.join("general-ledger.py");
    let stylesheet = stage.join("templates/general-ledger.css");
    let input = stage.join(format!("ledger-{}.html", args.ledger_size));
    let output = stage.join(format!("ledger-{}.pdf", args.ledger_size));

    fs::write(&generator, GENERATOR)?;
    fs::write(&stylesheet, STYLESHEET)?;

    let (binary, published_binary, build_command) = if let Some(binary) = external_binary {
        println!("[build] using {} (--binary; skipped)", binary.display());
        (binary.to_path_buf(), binary.to_path_buf(), None)
    } else {
        let ck = repo.join("ck");
        ensure!(
            ck.is_file(),
            "default build requires the paper-muncher `ck` launcher at {}",
            ck.display()
        );
        let package = stage.join("package");
        let published_package = target.join("package");
        let actual_build_command = direct_command_string(
            &ck,
            &[
                OsString::from("package"),
                OsString::from("--release"),
                OsString::from(format!("--prefix={}", package.display())),
                OsString::from("paper-muncher"),
            ],
        );
        let published_build_command = direct_command_string(
            &ck,
            &[
                OsString::from("package"),
                OsString::from("--release"),
                OsString::from(format!("--prefix={}", published_package.display())),
                OsString::from("paper-muncher"),
            ],
        );

        println!("[build] ck package --release");
        run_logged(
            Command::new(&ck)
                .arg("package")
                .arg("--release")
                .arg(format!("--prefix={}", package.display()))
                .arg("paper-muncher")
                .current_dir(repo),
            &stage.join("build.log"),
        )
        .with_context(|| format!("build command was `{actual_build_command}`"))?;

        let binary = package.join("bin/paper-muncher");
        ensure!(
            binary.is_file(),
            "ck package did not create {}",
            binary.display()
        );
        (
            binary,
            published_package.join("bin/paper-muncher"),
            Some(published_build_command),
        )
    };

    println!("[generator] {} entry ledger", args.ledger_size);
    run_logged(
        Command::new(python)
            .arg(&generator)
            .arg("--size")
            .arg(args.ledger_size.to_string())
            .arg("--output")
            .arg(&input)
            .current_dir(stage),
        &stage.join("generator.log"),
    )?;
    ensure!(
        input.is_file(),
        "ledger generator did not create {}",
        input.display()
    );

    let muncher_args = vec![
        input.as_os_str().to_owned(),
        OsString::from("-o"),
        output.as_os_str().to_owned(),
    ];
    let benchmark_command = direct_command_string(&binary, &muncher_args);
    let published_args = vec![
        target
            .join(format!("ledger-{}.html", args.ledger_size))
            .into_os_string(),
        OsString::from("-o"),
        target
            .join(format!("ledger-{}.pdf", args.ledger_size))
            .into_os_string(),
    ];
    let published_command = direct_command_string(&published_binary, &published_args);

    let metadata = metadata(
        args,
        repo,
        &published_binary,
        &published_command,
        build_command,
        selected,
        git,
        python,
        hyperfine,
        perf,
        heaptrack,
        heaptrack_print,
    )?;
    fs::write(
        stage.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;

    if let Some(hyperfine) = hyperfine {
        println!("[hyperfine] {} warmup, {} runs", args.warmup, args.runs);
        run_logged(
            Command::new(hyperfine)
                .arg("--shell=none")
                .arg("--warmup")
                .arg(args.warmup.to_string())
                .arg("--runs")
                .arg(args.runs.to_string())
                .arg("--export-json")
                .arg(stage.join("hyperfine.json"))
                .arg(&benchmark_command)
                .current_dir(repo),
            &stage.join("hyperfine.log"),
        )?;
    }

    if let Some(perf) = perf {
        println!("[perf] record + report");
        let mut perf_record = Command::new(perf);
        perf_record
            .arg("record")
            .arg(format!("--freq={}", args.perf_frequency))
            .arg("--call-graph")
            .arg(args.perf_call_graph.as_str())
            .arg("-q")
            .arg("-o")
            .arg(stage.join("perf.data"))
            .arg("--")
            .arg(&binary)
            .args(&muncher_args)
            .current_dir(repo);
        run_logged(&mut perf_record, &stage.join("perf.log"))?;

        capture_to_file(
            Command::new(perf)
                .arg("report")
                .arg("--stdio")
                .arg("--input")
                .arg(stage.join("perf.data"))
                .current_dir(repo),
            &stage.join("perf-report.txt"),
        )?;
    }

    if let (Some(heaptrack), Some(heaptrack_print)) = (heaptrack, heaptrack_print) {
        println!("[heaptrack] record + report");
        let mut heaptrack_command = Command::new(heaptrack);
        heaptrack_command
            .arg("-o")
            .arg(stage.join("heaptrack"))
            .arg(&binary)
            .args(&muncher_args)
            .current_dir(repo);
        run_logged(&mut heaptrack_command, &stage.join("heaptrack.log"))?;

        let heaptrack_data = stage.join("heaptrack.zst");
        ensure!(
            heaptrack_data.is_file(),
            "heaptrack did not create {}",
            heaptrack_data.display()
        );
        capture_to_file(
            Command::new(heaptrack_print).arg("-f").arg(&heaptrack_data),
            &stage.join("heaptrack.txt"),
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn metadata(
    args: &RunArgs,
    repo: &Path,
    binary: &Path,
    command: &str,
    build_command: Option<String>,
    selected: &BTreeSet<BenchTool>,
    git: &Path,
    python: &Path,
    hyperfine: Option<&Path>,
    perf: Option<&Path>,
    heaptrack: Option<&Path>,
    heaptrack_print: Option<&Path>,
) -> Result<Metadata> {
    let git_text = |arguments: &[&str]| -> Result<String> {
        checked_text(Command::new(git).args(arguments).current_dir(repo))
    };
    let git_commit = git_text(&["rev-parse", "HEAD"])?;
    let git_branch = git_text(&["branch", "--show-current"])?;
    let dirty = is_dirty_ignoring_artifacts(&git_text(&[
        "status",
        "--porcelain",
        "--untracked-files=all",
    ])?);

    let mut tools = BTreeMap::new();
    tools.insert("python".into(), version(python, &["--version"]));
    if let Some(hyperfine) = hyperfine {
        tools.insert("hyperfine".into(), version(hyperfine, &["--version"]));
    }
    if let Some(perf) = perf {
        tools.insert("perf".into(), version(perf, &["--version"]));
    }
    if let Some(heaptrack) = heaptrack {
        tools.insert("heaptrack".into(), version(heaptrack, &["--version"]));
    }
    if let Some(heaptrack_print) = heaptrack_print {
        tools.insert(
            "heaptrack_print".into(),
            version(heaptrack_print, &["--version"]),
        );
    }

    Ok(Metadata {
        git_commit,
        git_branch: if git_branch.is_empty() {
            "detached".into()
        } else {
            git_branch
        },
        dirty,
        ledger_size: args.ledger_size,
        runs: args.runs,
        warmup: args.warmup,
        perf_frequency: args.perf_frequency,
        perf_call_graph: args.perf_call_graph.as_str().into(),
        enabled_tools: selected
            .iter()
            .map(|tool| tool.as_str().to_owned())
            .collect(),
        command: command.into(),
        binary: binary.display().to_string(),
        build_command,
        timestamp_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        tools,
    })
}

fn selected_tools(args: &RunArgs) -> Result<BTreeSet<BenchTool>> {
    let mut selected: BTreeSet<_> = if args.only.is_empty() {
        BenchTool::ALL.into_iter().collect()
    } else {
        args.only.iter().copied().collect()
    };
    for tool in &args.skip {
        selected.remove(tool);
    }
    ensure!(
        !selected.is_empty(),
        "at least one profiler must be enabled"
    );
    Ok(selected)
}

fn optional_program(
    selected: &BTreeSet<BenchTool>,
    tool: BenchTool,
    program: &str,
) -> Result<Option<PathBuf>> {
    selected
        .contains(&tool)
        .then(|| require_program(program))
        .transpose()
}

fn version(program: &Path, args: &[&str]) -> String {
    let output = Command::new(program).args(args).output();
    match output {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                &output.stderr
            } else {
                &output.stdout
            };
            String::from_utf8_lossy(text)
                .lines()
                .next()
                .unwrap_or("unknown")
                .trim()
                .to_owned()
        }
        Err(_) => "unknown".into(),
    }
}

pub(crate) fn git_root() -> Result<PathBuf> {
    let git = require_program("git")?;
    let cwd = std::env::current_dir()?;
    let root = checked_text(
        Command::new(git)
            .arg("rev-parse")
            .arg("--show-toplevel")
            .current_dir(&cwd),
    )
    .context("paper-bencher must be run from inside a Git checkout")?;
    Ok(PathBuf::from(root))
}

fn publish(stage: &Path, target: &Path, force: bool) -> Result<()> {
    if !target.exists() {
        fs::rename(stage, target)?;
        return Ok(());
    }
    ensure!(force, "{} already exists", target.display());
    let backup = target.with_extension(format!("backup-{}", std::process::id()));
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    fs::rename(target, &backup)?;
    if let Err(error) = fs::rename(stage, target) {
        let _ = fs::rename(&backup, target);
        return Err(error.into());
    }
    fs::remove_dir_all(backup)?;
    Ok(())
}

pub(crate) fn validate_label(label: &str) -> Result<()> {
    ensure!(!label.is_empty(), "run label cannot be empty");
    ensure!(label != "." && label != "..", "invalid run label `{label}`");
    ensure!(
        label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
        "run label may contain only ASCII letters, digits, '.', '-', and '_'"
    );
    Ok(())
}

fn is_dirty_ignoring_artifacts(status: &str) -> bool {
    status.lines().any(|line| {
        let path = line.get(3..).unwrap_or(line).trim_matches('"');
        path != ".bench" && !path.starts_with(".bench/")
    })
}

#[cfg(test)]
mod tests {
    use super::{is_dirty_ignoring_artifacts, validate_label};

    #[test]
    fn labels_cannot_escape_the_artifact_directory() {
        assert!(validate_label("before").is_ok());
        assert!(validate_label("feature.flags-2").is_ok());
        assert!(validate_label("../before").is_err());
        assert!(validate_label("feature/flags").is_err());
        assert!(validate_label("..").is_err());
    }

    #[test]
    fn benchmark_artifacts_do_not_make_metadata_dirty() {
        assert!(!is_dirty_ignoring_artifacts("?? .bench/run/perf.data\n"));
        assert!(is_dirty_ignoring_artifacts(
            "?? .bench/run/perf.data\n M src/main.cpp\n"
        ));
    }
}
