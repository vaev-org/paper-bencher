use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "paper-bencher",
    version,
    about = "Build, profile, and compare paper-muncher revisions",
    long_about = "Build, profile, and compare paper-muncher revisions.\n\nBy default, each run builds the current checkout with `./ck package --release`, then executes Hyperfine, perf, and heaptrack sequentially. Results are stored under the current repository's `.bench/` directory.",
    after_long_help = "COMMAND GUIDE:
  run <LABEL>                         Benchmark the current checkout or --binary.
  compare <BEFORE> <AFTER>            Compare two runs already stored in .bench/.
  compare-refs <REF1> <REF2>           Build and compare two branches/tags/commits.
  compare-working-tree [BASE_REF]      Compare a clean ref with local changes.

BENCHMARK OPTIONS:
  -s, --ledger-size <N>                Number of ledger entries to generate.
                                        Default: 2000. Must be a multiple of 100.
  -r, --runs <N>                       Number of measured Hyperfine executions.
                                        Default: 5. Higher values improve confidence.
  -w, --warmup <N>                     Hyperfine executions discarded before measuring.
                                        Default: 1. Use 0 for a quick smoke test.
      --perf-frequency <HZ>            Sampling frequency used by `perf record`.
                                        Default: 997 samples/second.
      --perf-call-graph <MODE>         perf stack-unwinding mode: dwarf, fp, or lbr.
                                        Default: dwarf.
      --only <TOOLS>                   Run only selected tools. Comma-separated values:
                                        hyperfine, perf, heaptrack.
      --skip <TOOLS>                   Run all tools except the listed values.
                                        Cannot be combined with --only.
      --externs <MODE>                 Git comparisons only: dependency source.
                                        local (default) snapshots .cutekit/externs;
                                        fresh lets ck fetch refs from project.json.
  -b, --binary <PATH>                  `run` only: skip `ck package` and test PATH.
  -f, --force                          Replace an existing run/artifact label.

QUICK START:
  paper-bencher run before -s 2000 -w 1 -r 5
  # rebuild after making a change
  paper-bencher run after -s 2000 -w 1 -r 5
  paper-bencher compare before after

DIRECT GIT COMPARISONS:
  paper-bencher compare-refs main feature-branch --only hyperfine
  paper-bencher compare-refs v1.0.0 v1.1.0 -s 5000 --skip heaptrack
  paper-bencher compare-working-tree HEAD -r 10

COMMON RECIPES:
  # Fast smoke test: one timed execution, no warmup or profilers
  paper-bencher run smoke -s 100 -r 1 -w 0 --only hyperfine

  # CPU profiling only
  paper-bencher run cpu --only perf --perf-frequency 499

  # Benchmark an existing/custom executable without building
  paper-bencher run custom -b /path/to/paper-muncher --only hyperfine

OUTPUT:
  Runs:        .bench/<label>/
  Comparisons: .bench/compare-<before>-<after>/
  Includes metadata, logs, raw profiler data, summary.txt, and summary.md.

REQUIREMENTS:
  Always: git and python3.
  Default build: the checkout's ./ck and its build dependencies.
  Profilers: only the enabled commands among hyperfine, perf, heaptrack, and
  heaptrack_print need to be installed.

Options belong after the command, for example `paper-bencher run before -s 500`.
Use `paper-bencher <COMMAND> --help` for command-specific arguments and examples."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate a ledger and collect hyperfine, perf, and heaptrack profiles.
    Run(RunArgs),
    /// Compare two completed benchmark runs.
    Compare(CompareArgs),
    /// Build and compare two Git branches, tags, commits, or other refs.
    CompareRefs(CompareRefsArgs),
    /// Compare a clean Git ref with the current staged/unstaged working tree.
    CompareWorkingTree(CompareWorkingTreeArgs),
}

#[derive(Debug, Args)]
#[command(
    after_long_help = "EXAMPLES:\n  # Build and run all profilers\n  paper-bencher run before\n\n  # Timing only, with custom sample counts\n  paper-bencher run timing -s 2000 -w 2 -r 10 --only hyperfine\n\n  # Skip heap profiling\n  paper-bencher run cpu-and-time --skip heaptrack\n\n  # Use an existing binary instead of running ck package\n  paper-bencher run custom -b /path/to/paper-muncher\n\nEnabled profilers fail the run on error. Partial artifacts remain in a staging directory for diagnosis. Existing labeled runs require --force to replace."
)]
pub struct RunArgs {
    /// Short artifact label, for example `before`, `after`, or a branch name.
    pub label: String,

    /// Number of ledger entries to generate (must be a multiple of 100).
    #[arg(short = 's', long, default_value_t = 2_000, value_parser = clap::value_parser!(u64).range(100..))]
    pub ledger_size: u64,

    /// Number of timed Hyperfine executions included in the result.
    #[arg(short = 'r', long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub runs: u64,

    /// Number of untimed Hyperfine executions before measurement.
    #[arg(short = 'w', long, default_value_t = 1)]
    pub warmup: u64,

    /// perf record samples per second; only used when perf is enabled.
    #[arg(long, default_value_t = 997, value_parser = clap::value_parser!(u64).range(1..))]
    pub perf_frequency: u64,

    /// perf stack-unwinding mode: dwarf, fp (frame pointer), or lbr.
    #[arg(long, value_enum, default_value_t = PerfCallGraph::Dwarf)]
    pub perf_call_graph: PerfCallGraph,

    /// Run only these tools: hyperfine, perf, and/or heaptrack.
    #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "skip")]
    pub only: Vec<BenchTool>,

    /// Skip these tools; cannot be combined with --only.
    #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "only")]
    pub skip: Vec<BenchTool>,

    /// Skip `ck package` and benchmark this executable instead.
    #[arg(short = 'b', long)]
    pub binary: Option<PathBuf>,

    /// Replace an existing run with the same label after the new run succeeds.
    #[arg(short = 'f', long)]
    pub force: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PerfCallGraph {
    Dwarf,
    Fp,
    Lbr,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
pub enum BenchTool {
    Hyperfine,
    Perf,
    Heaptrack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ExternsMode {
    /// Copy the current checkout's .cutekit/externs into temporary worktrees.
    Local,
    /// Let ck fetch dependencies declared by each ref's project.json.
    Fresh,
}

impl BenchTool {
    pub const ALL: [Self; 3] = [Self::Hyperfine, Self::Perf, Self::Heaptrack];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hyperfine => "hyperfine",
            Self::Perf => "perf",
            Self::Heaptrack => "heaptrack",
        }
    }
}

impl PerfCallGraph {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dwarf => "dwarf",
            Self::Fp => "fp",
            Self::Lbr => "lbr",
        }
    }
}

#[derive(Debug, Args)]
#[command(
    after_long_help = "EXAMPLE:\n  paper-bencher compare main feature\n\nBoth runs must have matching ledger size and matching enabled-profiler settings."
)]
pub struct CompareArgs {
    /// Baseline run label.
    pub before: String,

    /// Candidate run label.
    pub after: String,
}

#[derive(Debug, Args)]
#[command(
    after_long_help = "EXAMPLES:\n  paper-bencher compare-refs main feature/remove-flags -s 2000 -r 5\n  paper-bencher compare-refs v1.0.0 v1.1.0 --externs fresh\n\nBoth refs are checked out in temporary detached worktrees. By default, the current checkout's .cutekit/externs is copied to both worktrees so coupled Karm changes stay fixed and comparable. Use --externs fresh to resolve each ref's project.json instead. The current checkout is not switched or modified."
)]
pub struct CompareRefsArgs {
    /// Baseline branch, tag, commit, or Git revision.
    pub before_ref: String,

    /// Candidate branch, tag, commit, or Git revision.
    pub after_ref: String,

    #[command(flatten)]
    pub options: RefOptions,
}

#[derive(Debug, Args)]
#[command(
    after_long_help = "EXAMPLES:\n  paper-bencher compare-working-tree HEAD --only hyperfine -r 10\n  paper-bencher compare-working-tree main --externs fresh\n\nThe baseline ref is built in a temporary detached worktree. The candidate is built from the current checkout, including staged and unstaged tracked changes. By default, local .cutekit/externs is snapshotted into the clean worktree, so both builds use the Karm state you selected manually."
)]
pub struct CompareWorkingTreeArgs {
    /// Clean baseline revision (defaults to HEAD).
    #[arg(default_value = "HEAD")]
    pub base_ref: String,

    #[command(flatten)]
    pub options: RefOptions,
}

#[derive(Clone, Debug, Args)]
pub struct RefOptions {
    /// Number of ledger entries to generate (must be a multiple of 100).
    #[arg(short = 's', long, default_value_t = 2_000, value_parser = clap::value_parser!(u64).range(100..))]
    pub ledger_size: u64,

    /// Number of timed Hyperfine executions included in each result.
    #[arg(short = 'r', long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub runs: u64,

    /// Number of untimed Hyperfine executions before each measurement.
    #[arg(short = 'w', long, default_value_t = 1)]
    pub warmup: u64,

    /// perf record samples per second; only used when perf is enabled.
    #[arg(long, default_value_t = 997, value_parser = clap::value_parser!(u64).range(1..))]
    pub perf_frequency: u64,

    /// perf stack-unwinding mode: dwarf, fp (frame pointer), or lbr.
    #[arg(long, value_enum, default_value_t = PerfCallGraph::Dwarf)]
    pub perf_call_graph: PerfCallGraph,

    /// Run only these tools: hyperfine, perf, and/or heaptrack.
    #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "skip")]
    pub only: Vec<BenchTool>,

    /// Skip these tools; cannot be combined with --only.
    #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "only")]
    pub skip: Vec<BenchTool>,

    /// Dependency source: snapshot local externs, or let ck fetch fresh ones.
    #[arg(long, value_enum, default_value_t = ExternsMode::Local)]
    pub externs: ExternsMode,

    /// Replace existing generated run labels after new runs succeed.
    #[arg(short = 'f', long)]
    pub force: bool,
}

impl RefOptions {
    pub fn run_args(&self, label: String) -> RunArgs {
        RunArgs {
            label,
            ledger_size: self.ledger_size,
            runs: self.runs,
            warmup: self.warmup,
            perf_frequency: self.perf_frequency,
            perf_call_graph: self.perf_call_graph,
            only: self.only.clone(),
            skip: self.skip.clone(),
            binary: None,
            force: self.force,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Cli, Command, ExternsMode};

    #[test]
    fn accepts_short_benchmark_flags() {
        let cli = Cli::try_parse_from([
            "paper-bencher",
            "run",
            "before",
            "-s",
            "500",
            "-w",
            "2",
            "-r",
            "10",
            "--perf-frequency",
            "499",
            "--perf-call-graph",
            "fp",
            "--only",
            "hyperfine,perf",
        ])
        .unwrap();

        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.ledger_size, 500);
        assert_eq!(args.warmup, 2);
        assert_eq!(args.runs, 10);
        assert_eq!(args.perf_frequency, 499);
        assert_eq!(args.perf_call_graph.as_str(), "fp");
        assert_eq!(args.only.len(), 2);
        assert!(args.skip.is_empty());
    }

    #[test]
    fn only_and_skip_conflict() {
        assert!(
            Cli::try_parse_from([
                "paper-bencher",
                "run",
                "before",
                "--only",
                "perf",
                "--skip",
                "heaptrack",
            ])
            .is_err()
        );
    }

    #[test]
    fn top_level_help_explains_common_benchmark_flags() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("-s, --ledger-size <N>"));
        assert!(help.contains("Number of ledger entries to generate"));
        assert!(help.contains("-r, --runs <N>"));
        assert!(help.contains("--only <TOOLS>"));
        assert!(help.contains("--externs <MODE>"));
        assert!(help.contains("compare-working-tree [BASE_REF]"));
    }

    #[test]
    fn git_comparison_accepts_fresh_externs() {
        let cli = Cli::try_parse_from([
            "paper-bencher",
            "compare-working-tree",
            "HEAD",
            "--externs",
            "fresh",
        ])
        .unwrap();

        let Command::CompareWorkingTree(args) = cli.command else {
            panic!("expected compare-working-tree command");
        };
        assert_eq!(args.options.externs, ExternsMode::Fresh);
    }
}
