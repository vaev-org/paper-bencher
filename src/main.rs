mod cli;
mod compare;
mod model;
mod process;
mod refs;
mod runner;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

fn main() {
    if let Err(error) = try_main() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => runner::run(args),
        Command::Compare(args) => compare::compare(args),
        Command::CompareRefs(args) => refs::compare_refs(args),
        Command::CompareWorkingTree(args) => refs::compare_working_tree(args),
    }
}
