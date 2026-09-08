use std::{
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, ensure};

use crate::process::require_program;

pub fn open_profile(profile: &Path) -> Result<u32> {
    ensure!(
        profile.is_file(),
        "heaptrack profile not found: {}",
        profile.display()
    );
    let heaptrack_gui = require_program("heaptrack_gui")?;
    let setsid = require_program("setsid")?;
    let child = Command::new(setsid)
        .arg("--fork")
        .arg(heaptrack_gui)
        .arg(profile)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "cannot open {} in a detached heaptrack_gui process",
                profile.display()
            )
        })?;
    Ok(child.id())
}

pub fn open_profiles(before: &Path, after: &Path) -> Result<(u32, u32)> {
    Ok((open_profile(before)?, open_profile(after)?))
}
