use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, bail, ensure};

use crate::{
    cli::CompareArgs,
    model::{HeapSummary, HyperfineExport, HyperfineResult, Metadata},
    process::{capture_to_file, require_program},
    runner::validate_label,
};

pub fn compare(args: CompareArgs) -> Result<()> {
    validate_label(&args.before)?;
    validate_label(&args.after)?;
    ensure!(
        args.before != args.after,
        "cannot compare a run with itself"
    );

    let git = require_program("git")?;
    let root = Command::new(git)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("paper-bencher must be run from inside a Git checkout")?;
    ensure!(
        root.status.success(),
        "paper-bencher must be run from inside a Git checkout"
    );
    let repo = Path::new(std::str::from_utf8(&root.stdout)?.trim()).to_path_buf();
    let bench_root = repo.join(".bench");
    let before_dir = bench_root.join(&args.before);
    let after_dir = bench_root.join(&args.after);

    let before_meta: Metadata = read_json(&before_dir.join("metadata.json"))?;
    let after_meta: Metadata = read_json(&after_dir.join("metadata.json"))?;
    ensure!(
        before_meta.ledger_size == after_meta.ledger_size,
        "ledger sizes differ: {} has {}, {} has {}",
        args.before,
        before_meta.ledger_size,
        args.after,
        after_meta.ledger_size
    );
    ensure!(
        before_meta.enabled_tools == after_meta.enabled_tools,
        "enabled profilers differ: {} has {:?}, {} has {:?}",
        args.before,
        before_meta.enabled_tools,
        args.after,
        after_meta.enabled_tools
    );
    let enabled = |tool: &str| before_meta.enabled_tools.iter().any(|item| item == tool);
    if enabled("hyperfine") {
        ensure!(
            before_meta.runs == after_meta.runs && before_meta.warmup == after_meta.warmup,
            "hyperfine settings differ between runs (runs/warmup: {}/{} vs {}/{})",
            before_meta.runs,
            before_meta.warmup,
            after_meta.runs,
            after_meta.warmup
        );
    }
    if enabled("perf") {
        ensure!(
            before_meta.perf_frequency == after_meta.perf_frequency
                && before_meta.perf_call_graph == after_meta.perf_call_graph,
            "perf settings differ between runs (frequency/call graph: {}/{} vs {}/{})",
            before_meta.perf_frequency,
            before_meta.perf_call_graph,
            after_meta.perf_frequency,
            after_meta.perf_call_graph
        );
    }

    let before_time = enabled("hyperfine")
        .then(|| read_hyperfine(&before_dir.join("hyperfine.json")))
        .transpose()?;
    let after_time = enabled("hyperfine")
        .then(|| read_hyperfine(&after_dir.join("hyperfine.json")))
        .transpose()?;
    let before_heap = enabled("heaptrack")
        .then(|| parse_heaptrack(&fs::read_to_string(before_dir.join("heaptrack.txt"))?))
        .transpose()?;
    let after_heap = enabled("heaptrack")
        .then(|| parse_heaptrack(&fs::read_to_string(after_dir.join("heaptrack.txt"))?))
        .transpose()?;

    let report = bench_root.join(format!("compare-{}-{}", args.before, args.after));
    let stage = bench_root.join(format!(
        ".compare-{}-{}-{}-staging",
        args.before,
        args.after,
        std::process::id()
    ));
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    fs::create_dir_all(&stage)?;

    if enabled("perf") {
        let perf = require_program("perf")?;
        capture_to_file(
            Command::new(perf)
                .arg("diff")
                .arg(before_dir.join("perf.data"))
                .arg(after_dir.join("perf.data")),
            &stage.join("perf-diff.txt"),
        )?;
    }

    if enabled("heaptrack") {
        let heaptrack_print = require_program("heaptrack_print")?;
        capture_to_file(
            Command::new(heaptrack_print)
                .arg("-f")
                .arg(after_dir.join("heaptrack.zst"))
                .arg("--diff")
                .arg(before_dir.join("heaptrack.zst")),
            &stage.join("heaptrack-diff.txt"),
        )?;
    }

    let data = Comparison {
        before_label: &args.before,
        after_label: &args.after,
        before_meta: &before_meta,
        after_meta: &after_meta,
        before_time,
        after_time,
        before_heap,
        after_heap,
    };
    let text = render_text(&data);
    fs::write(stage.join("summary.txt"), &text)?;
    fs::write(stage.join("summary.md"), render_markdown(&data))?;

    replace_directory(&stage, &report)?;
    print!("{text}");
    if enabled("perf") {
        println!("\nperf diff: {}", report.join("perf-diff.txt").display());
    }
    if enabled("heaptrack") {
        println!(
            "heaptrack diff: {}",
            report.join("heaptrack-diff.txt").display()
        );
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn read_hyperfine(path: &Path) -> Result<HyperfineResult> {
    let mut export: HyperfineExport = read_json(path)?;
    ensure!(
        export.results.len() == 1,
        "expected exactly one hyperfine result in {}",
        path.display()
    );
    let result = export.results.remove(0);
    ensure!(
        result.exit_codes.iter().all(|code| *code == 0),
        "{} contains failed benchmark runs",
        path.display()
    );
    Ok(result)
}

fn parse_heaptrack(text: &str) -> Result<HeapSummary> {
    let allocations = value_after(text, "calls to allocation functions:")?
        .split_whitespace()
        .next()
        .context("missing allocation count")?
        .replace([',', '_'], "")
        .parse::<u64>()?;
    let peak = value_after(text, "peak heap memory consumption:")?
        .split_whitespace()
        .next()
        .context("missing peak heap value")?;

    Ok(HeapSummary {
        allocations,
        peak_heap_bytes: parse_bytes(peak)?,
    })
}

fn value_after<'a>(text: &'a str, prefix: &str) -> Result<&'a str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(prefix))
        .map(str::trim)
        .with_context(|| format!("heaptrack output has no `{prefix}` line"))
}

fn parse_bytes(value: &str) -> Result<f64> {
    let split = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let number: f64 = value[..split].parse()?;
    let unit = &value[split..];
    let factor = match unit {
        "" | "B" => 1.0,
        "K" | "KiB" => 1024.0,
        "M" | "MiB" => 1024.0_f64.powi(2),
        "G" | "GiB" => 1024.0_f64.powi(3),
        "T" | "TiB" => 1024.0_f64.powi(4),
        _ => bail!("unsupported heaptrack byte value `{value}`"),
    };
    Ok(number * factor)
}

struct Comparison<'a> {
    before_label: &'a str,
    after_label: &'a str,
    before_meta: &'a Metadata,
    after_meta: &'a Metadata,
    before_time: Option<HyperfineResult>,
    after_time: Option<HyperfineResult>,
    before_heap: Option<HeapSummary>,
    after_heap: Option<HeapSummary>,
}

fn render_text(data: &Comparison<'_>) -> String {
    let mut output = format!(
        "paper-muncher benchmark\nLedger: {} entries\nProfilers: {}\n\n{: <22} {:>13} {:>13} {:>12}\n",
        data.before_meta.ledger_size,
        data.before_meta.enabled_tools.join(", "),
        "metric",
        data.before_label,
        data.after_label,
        "change"
    );
    if let (Some(before), Some(after)) = (&data.before_time, &data.after_time) {
        push_text_row(
            &mut output,
            "Runtime mean",
            seconds(before.mean),
            seconds(after.mean),
            percent(before.mean, after.mean),
        );
        push_text_row(
            &mut output,
            "Runtime median",
            seconds(before.median),
            seconds(after.median),
            percent(before.median, after.median),
        );
    }
    if let (Some(before), Some(after)) = (&data.before_heap, &data.after_heap) {
        push_text_row(
            &mut output,
            "Allocations",
            grouped(before.allocations),
            grouped(after.allocations),
            percent(before.allocations as f64, after.allocations as f64),
        );
        push_text_row(
            &mut output,
            "Peak heap",
            mebibytes(before.peak_heap_bytes),
            mebibytes(after.peak_heap_bytes),
            percent(before.peak_heap_bytes, after.peak_heap_bytes),
        );
    }
    output.push_str(&format!(
        "\nGit:\n  {}: {} ({}){}\n  {}: {} ({}){}\n",
        data.before_label,
        short_commit(&data.before_meta.git_commit),
        data.before_meta.git_branch,
        dirty_suffix(data.before_meta.dirty),
        data.after_label,
        short_commit(&data.after_meta.git_commit),
        data.after_meta.git_branch,
        dirty_suffix(data.after_meta.dirty),
    ));
    output
}

fn render_markdown(data: &Comparison<'_>) -> String {
    let mut output = format!(
        "### Benchmark — {} entry ledger\n\nProfilers: {}\n\n| Metric | {} | {} | Difference |\n|---|---:|---:|---:|\n",
        data.before_meta.ledger_size,
        data.before_meta.enabled_tools.join(", "),
        data.before_label,
        data.after_label,
    );
    if let (Some(before), Some(after)) = (&data.before_time, &data.after_time) {
        push_markdown_row(
            &mut output,
            "Runtime (mean)",
            seconds(before.mean),
            seconds(after.mean),
            percent(before.mean, after.mean),
        );
        push_markdown_row(
            &mut output,
            "Runtime (median)",
            seconds(before.median),
            seconds(after.median),
            percent(before.median, after.median),
        );
    }
    if let (Some(before), Some(after)) = (&data.before_heap, &data.after_heap) {
        push_markdown_row(
            &mut output,
            "Allocations",
            compact_count(before.allocations),
            compact_count(after.allocations),
            percent(before.allocations as f64, after.allocations as f64),
        );
        push_markdown_row(
            &mut output,
            "Peak heap",
            mebibytes(before.peak_heap_bytes),
            mebibytes(after.peak_heap_bytes),
            percent(before.peak_heap_bytes, after.peak_heap_bytes),
        );
    }
    if data
        .before_meta
        .enabled_tools
        .iter()
        .any(|tool| tool == "perf")
    {
        output.push_str("\n`perf diff` is available in the benchmark artifacts.\n");
    }
    if data
        .before_meta
        .enabled_tools
        .iter()
        .any(|tool| tool == "heaptrack")
    {
        output.push_str("\nFull heaptrack results are available in the benchmark artifacts.\n");
    }
    output
}

fn push_text_row(output: &mut String, metric: &str, before: String, after: String, change: String) {
    output.push_str(&format!(
        "{metric:<22} {before:>13} {after:>13} {change:>12}\n"
    ));
}

fn push_markdown_row(
    output: &mut String,
    metric: &str,
    before: String,
    after: String,
    change: String,
) {
    output.push_str(&format!(
        "| {metric} | {before} | {after} | **{change}** |\n"
    ));
}

fn percent(before: f64, after: f64) -> String {
    if before == 0.0 {
        "n/a".into()
    } else {
        format!("{:+.2}%", (after - before) / before * 100.0)
    }
}

fn seconds(value: f64) -> String {
    format!("{value:.3} s")
}

fn mebibytes(bytes: f64) -> String {
    format!("{:.1} MiB", bytes / 1024.0_f64.powi(2))
}

fn compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.2} M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.2} K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn short_commit(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

fn dirty_suffix(dirty: bool) -> &'static str {
    if dirty { " dirty" } else { "" }
}

fn replace_directory(stage: &Path, target: &Path) -> Result<()> {
    let backup = target.with_extension(format!("backup-{}", std::process::id()));
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    if target.exists() {
        fs::rename(target, &backup)?;
    }
    if let Err(error) = fs::rename(stage, target) {
        if backup.exists() {
            let _ = fs::rename(&backup, target);
        }
        return Err(error.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{grouped, parse_bytes, parse_heaptrack, percent};

    #[test]
    fn parses_heaptrack_summary() {
        let text = "calls to allocation functions: 8,291,212 (10/s)\n\
                    peak heap memory consumption: 312.4M\n";
        let summary = parse_heaptrack(text).unwrap();
        assert_eq!(summary.allocations, 8_291_212);
        assert_eq!(summary.peak_heap_bytes, 312.4 * 1024.0 * 1024.0);
    }

    #[test]
    fn parses_heaptrack_units() {
        assert_eq!(parse_bytes("42B").unwrap(), 42.0);
        assert_eq!(parse_bytes("1.5K").unwrap(), 1536.0);
        assert_eq!(parse_bytes("2MiB").unwrap(), 2.0 * 1024.0 * 1024.0);
    }

    #[test]
    fn formats_changes_and_counts() {
        assert_eq!(percent(100.0, 84.69), "-15.31%");
        assert_eq!(percent(0.0, 1.0), "n/a");
        assert_eq!(grouped(8_291_212), "8,291,212");
    }
}
