use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, ensure};

use crate::{
    cli::{CompareArgs, CompareRefsArgs, CompareWorkingTreeArgs, ExternsMode},
    compare,
    process::{checked_output, checked_text, require_program},
    runner,
};

pub fn compare_refs(args: CompareRefsArgs) -> Result<()> {
    let repo = runner::git_root()?;
    let before_commit = resolve_ref(&repo, &args.before_ref)?;
    let after_commit = resolve_ref(&repo, &args.after_ref)?;
    let before_label = generated_label("before", &args.before_ref, &before_commit);
    let after_label = generated_label("after", &args.after_ref, &after_commit);
    let before_worktree = temporary_worktree("before");
    let after_worktree = temporary_worktree("after");

    let result = (|| {
        add_worktree(&repo, &before_worktree, &args.before_ref)?;
        add_worktree(&repo, &after_worktree, &args.after_ref)?;
        prepare_externs(&repo, &before_worktree, args.options.externs)?;
        prepare_externs(&repo, &after_worktree, args.options.externs)?;
        runner::run_at(
            args.options.run_args(before_label.clone()),
            &before_worktree,
            &repo,
        )?;
        runner::run_at(
            args.options.run_args(after_label.clone()),
            &after_worktree,
            &repo,
        )?;
        compare::compare_at(
            CompareArgs {
                before: before_label,
                after: after_label,
            },
            &repo,
            false,
        )
    })();

    cleanup_worktree(&repo, &before_worktree);
    cleanup_worktree(&repo, &after_worktree);
    result
}

pub fn compare_working_tree(args: CompareWorkingTreeArgs) -> Result<()> {
    let repo = runner::git_root()?;
    let base_commit = resolve_ref(&repo, &args.base_ref)?;
    let current_commit = resolve_ref(&repo, "HEAD")?;
    let base_label = generated_label("base", &args.base_ref, &base_commit);
    let working_label = generated_label("working-tree", "current", &current_commit);
    let base_worktree = temporary_worktree("base");

    let result = (|| {
        add_worktree(&repo, &base_worktree, &args.base_ref)?;
        prepare_externs(&repo, &base_worktree, args.options.externs)?;
        runner::run_at(
            args.options.run_args(base_label.clone()),
            &base_worktree,
            &repo,
        )?;
        runner::run_at(args.options.run_args(working_label.clone()), &repo, &repo)?;
        compare::compare_at(
            CompareArgs {
                before: base_label,
                after: working_label,
            },
            &repo,
            false,
        )
    })();

    cleanup_worktree(&repo, &base_worktree);
    result
}

fn resolve_ref(repo: &Path, revision: &str) -> Result<String> {
    let git = require_program("git")?;
    checked_text(
        Command::new(git)
            .arg("rev-parse")
            .arg("--verify")
            .arg(format!("{revision}^{{commit}}"))
            .current_dir(repo),
    )
    .with_context(|| format!("cannot resolve Git revision `{revision}`"))
}

fn add_worktree(repo: &Path, path: &Path, revision: &str) -> Result<()> {
    ensure!(
        !path.exists(),
        "temporary path already exists: {}",
        path.display()
    );
    let git = require_program("git")?;
    println!("[git] preparing `{revision}` in {}", path.display());
    checked_output(
        Command::new(git)
            .arg("worktree")
            .arg("add")
            .arg("--detach")
            .arg(path)
            .arg(revision)
            .current_dir(repo),
    )?;
    Ok(())
}

fn prepare_externs(repo: &Path, worktree: &Path, mode: ExternsMode) -> Result<()> {
    if mode == ExternsMode::Fresh {
        println!("[deps] fresh mode; ck will resolve project.json externs");
        return Ok(());
    }

    let source = repo.join(".cutekit/externs");
    ensure!(
        source.is_dir(),
        "local extern snapshot requested, but {} does not exist; run ck once or pass --externs fresh",
        source.display()
    );

    let cutekit = worktree.join(".cutekit");
    std::fs::create_dir_all(&cutekit)
        .with_context(|| format!("cannot create {}", cutekit.display()))?;
    let destination = cutekit.join("externs");
    let cp = require_program("cp")?;
    println!("[deps] snapshotting local .cutekit/externs");
    checked_output(
        Command::new(cp)
            .arg("--archive")
            .arg("--reflink=auto")
            .arg(&source)
            .arg(&destination),
    )
    .with_context(|| {
        format!(
            "cannot snapshot {} into {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn cleanup_worktree(repo: &Path, path: &Path) {
    if !path.exists() {
        return;
    }
    let Ok(git) = require_program("git") else {
        return;
    };
    let status = Command::new(git)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(path)
        .current_dir(repo)
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        eprintln!(
            "warning: could not remove temporary worktree {}; remove it manually",
            path.display()
        );
    }
}

fn temporary_worktree(role: &str) -> PathBuf {
    std::env::temp_dir().join(format!("paper-bencher-{}-{role}", std::process::id()))
}

fn generated_label(role: &str, revision: &str, commit: &str) -> String {
    let revision: String = revision
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();
    let revision = revision.trim_matches(['-', '.']);
    let revision = if revision.is_empty() { "ref" } else { revision };
    format!("{role}-{revision}-{}", &commit[..commit.len().min(8)])
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{ExternsMode, generated_label, prepare_externs};

    #[test]
    fn generated_labels_are_safe_and_readable() {
        assert_eq!(
            generated_label(
                "after",
                "feature/remove-flags",
                "a0e6c4b7697609f9ca6847d0e844088ce67470da"
            ),
            "after-feature-remove-flags-a0e6c4b7"
        );
    }

    #[test]
    fn local_externs_are_snapshotted() {
        let root =
            std::env::temp_dir().join(format!("paper-bencher-externs-test-{}", std::process::id()));
        let repo = root.join("repo");
        let worktree = root.join("worktree");
        fs::create_dir_all(repo.join(".cutekit/externs/skift/karm")).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(
            repo.join(".cutekit/externs/skift/karm/revision"),
            "test-commit",
        )
        .unwrap();

        prepare_externs(&repo, &worktree, ExternsMode::Local).unwrap();

        assert_eq!(
            fs::read_to_string(worktree.join(".cutekit/externs/skift/karm/revision")).unwrap(),
            "test-commit"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
