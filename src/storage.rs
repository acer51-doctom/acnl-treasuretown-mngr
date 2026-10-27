use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let tmp_name = format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp"),
        std::process::id()
    );
    let tmp_path = parent.join(tmp_name);
    std::fs::write(&tmp_path, contents)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn validated_town_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("town name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("invalid town name: path traversal is not allowed");
    }
    Ok(())
}

pub fn lock_file(path: &Path) -> Result<std::fs::File> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "could not acquire lock at {} (another operation may be in progress)",
                path.display()
            )
        })?;
    Ok(file)
}

pub fn release_lock(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}

pub fn copy_with_backup(from: &Path, to: &Path, backup_root: &Path) -> Result<()> {
    if to.exists() {
        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        let backup = backup_root.join(format!(
            "{}-{timestamp}",
            to.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("save.bak")
        ));
        std::fs::create_dir_all(backup_root)?;
        std::fs::copy(to, &backup)?;
    }
    std::fs::create_dir_all(
        to.parent()
            .with_context(|| format!("{} has no parent", to.display()))?,
    )?;
    std::fs::copy(from, to)?;
    Ok(())
}

pub fn latest_snapshot_dir(snapshot_root: &Path) -> Result<PathBuf> {
    let mut entries = std::fs::read_dir(snapshot_root)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();

    entries.sort_by_key(|entry| entry.file_name());
    entries
        .pop()
        .map(|entry| entry.path())
        .context("no snapshots found")
}
