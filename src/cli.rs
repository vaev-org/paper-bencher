use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "paper-bencher",
    version,
    about = "Benchmark two paper-muncher builds"
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
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Short artifact label, for example `before`, `after`, or a branch name.
    pub label: String,

    /// Number of ledger entries requested from the bundled generator.
    #[arg(short = 's', long, default_value_t = 2_000, value_parser = clap::value_parser!(u64).range(100..))]
    pub ledger_size: u64,

    /// Number of measured hyperfine runs.
    #[arg(short = 'r', long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub runs: u64,

    /// Number of hyperfine warmup runs.
    #[arg(short = 'w', long, default_value_t = 1)]
    pub warmup: u64,

    /// Sampling frequency passed to `perf record`.
    #[arg(long, default_value_t = 997, value_parser = clap::value_parser!(u64).range(1..))]
    pub perf_frequency: u64,

    /// Call graph mode passed to `perf record`.
    #[arg(long, value_enum, default_value_t = PerfCallGraph::Dwarf)]
    pub perf_call_graph: PerfCallGraph,

    /// Run only these profilers (comma-separated or repeated).
    #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "skip")]
    pub only: Vec<BenchTool>,

    /// Skip these profilers (comma-separated or repeated).
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
pub struct CompareArgs {
    /// Baseline run label.
    pub before: String,

    /// Candidate run label.
    pub after: String,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command};

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
}
