use crate::config::{AppConfig, AppPaths};
use crate::hosting::{BackendConfig, HostingBackend};
use crate::process_monitor;
use crate::storage;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TownConfig {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub backend: BackendConfig,
    pub session_limit_minutes: Option<u64>,
}

impl TownConfig {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            backend: BackendConfig::default(),
            session_limit_minutes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRuntime {
    pub state: SessionState,
    pub backend_kind: String,
    pub pid: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub restart_attempts: u32,
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self {
            state: SessionState::Stopped,
            backend_kind: "manual".into(),
            pid: None,
            started_at: None,
            ended_at: None,
            last_error: None,
            restart_attempts: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub files: Vec<SnapshotFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFile {
    pub name: String,
    pub sha256: String,
}

pub struct TownManager {
    paths: AppPaths,
    config: AppConfig,
}

impl TownManager {
    pub fn new(paths: AppPaths, config: AppConfig) -> Self {
        Self { paths, config }
    }

    fn town_paths(&self, name: &str) -> Result<TownPaths> {
        storage::validated_town_name(name)?;
        Ok(TownPaths::new(&self.paths.towns_dir, name))
    }

    pub fn doctor(&self) -> Result<()> {
        self.paths.ensure_layout()?;
        println!("data root: {}", self.paths.root.display());
        println!("config: {}", self.paths.config_file.display());
        println!("towns dir: {}", self.paths.towns_dir.display());
        println!("templates dir: {}", self.paths.templates_dir.display());
        println!("max active towns: {}", self.config.max_active_towns);
        println!(
            "default session limit: {} minutes",
            self.config.default_session_limit_minutes
        );
        println!("doctor: OK");
        Ok(())
    }

    pub fn create_town(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        if tp.root.exists() {
            bail!("town '{name}' already exists");
        }
        tp.ensure_dirs()?;
        let cfg = TownConfig::new(name.to_owned());
        storage::atomic_write(&tp.config_file, toml::to_string_pretty(&cfg)?.as_bytes())?;
        storage::atomic_write(
            &tp.runtime_file,
            toml::to_string_pretty(&SessionRuntime::default())?.as_bytes(),
        )?;
        println!("Created town '{name}' at {}", tp.root.display());
        Ok(())
    }

    pub fn list_towns(&self) -> Result<()> {
        self.paths.ensure_layout()?;
        let mut found = false;
        for entry in std::fs::read_dir(&self.paths.towns_dir)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name();
            println!("{}", name.to_string_lossy());
            found = true;
        }
        if !found {
            println!("No towns registered.");
        }
        Ok(())
    }

    pub fn town_info(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let cfg = self.load_town_config(&tp)?;
        let runtime = self.load_runtime(&tp)?;
        println!("name: {}", cfg.name);
        println!("id: {}", cfg.id);
        println!("backend: {}", backend_kind(&cfg.backend));
        println!(
            "session_limit_minutes: {}",
            cfg.session_limit_minutes
                .unwrap_or(self.config.default_session_limit_minutes)
        );
        println!("state: {:?}", runtime.state);
        println!("pid: {:?}", runtime.pid);
        println!("started_at: {:?}", runtime.started_at);
        println!("last_error: {:?}", runtime.last_error);
        Ok(())
    }

    pub fn delete_town(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let runtime = self.load_runtime(&tp)?;
        if runtime.state == SessionState::Running || runtime.state == SessionState::Starting {
            bail!("cannot delete running town '{name}'. stop it first");
        }

        std::fs::remove_dir_all(&tp.root)
            .with_context(|| format!("failed to remove {}", tp.root.display()))?;
        println!("Deleted town '{name}'");
        Ok(())
    }

    pub fn import_save(&self, name: &str, path: &Path) -> Result<()> {
        let tp = self.town_paths(name)?;
        let _guard = tp.acquire_lock()?;
        let file_name = path
            .file_name()
            .and_then(|f| f.to_str())
            .context("save path must be a file")?;
        if file_name != "garden.dat" && file_name != "garden_plus.dat" {
            bail!("only garden.dat and garden_plus.dat can be imported");
        }
        if !path.exists() {
            bail!("save file '{}' does not exist", path.display());
        }

        let target = tp.saves_dir.join(file_name);
        storage::copy_with_backup(path, &target, &tp.saves_backup_dir)?;
        println!("Imported save '{}' into town '{name}'", file_name);
        Ok(())
    }

    pub fn backup_town(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let _guard = tp.acquire_lock()?;
        let snapshot_id = Utc::now().format("%Y%m%d%H%M%S").to_string();
        let snapshot_dir = tp.snapshots_dir.join(&snapshot_id);
        let files_dir = snapshot_dir.join("files");
        std::fs::create_dir_all(&files_dir)?;

        let mut files = Vec::new();
        for fname in ["garden.dat", "garden_plus.dat"] {
            let src = tp.saves_dir.join(fname);
            if src.exists() {
                let dst = files_dir.join(fname);
                std::fs::copy(&src, &dst)?;
                let hash = storage::sha256_file(&dst)?;
                files.push(SnapshotFile {
                    name: fname.into(),
                    sha256: hash,
                });
            }
        }

        if files.is_empty() {
            bail!("no save files available to snapshot for town '{name}'");
        }

        let meta = SnapshotMeta {
            id: snapshot_id.clone(),
            created_at: Utc::now(),
            files,
        };
        let meta_toml = toml::to_string_pretty(&meta)?;
        storage::atomic_write(&snapshot_dir.join("snapshot.toml"), meta_toml.as_bytes())?;
        println!("Created snapshot '{snapshot_id}' for town '{name}'");
        Ok(())
    }

    pub fn list_snapshots(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        if !tp.snapshots_dir.exists() {
            println!("No snapshots for '{name}'");
            return Ok(());
        }

        let mut found = false;
        for entry in std::fs::read_dir(&tp.snapshots_dir)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }
            let meta_file = entry.path().join("snapshot.toml");
            if meta_file.exists() {
                let raw = std::fs::read_to_string(meta_file)?;
                let meta: SnapshotMeta = toml::from_str(&raw)?;
                println!("{} {} file(s)", meta.id, meta.files.len());
                found = true;
            }
        }
        if !found {
            println!("No snapshots for '{name}'");
        }
        Ok(())
    }

    pub fn restore_snapshot(&self, name: &str, snapshot: &str) -> Result<()> {
        if snapshot.contains('/') || snapshot.contains('\\') || snapshot.contains("..") {
            bail!("invalid snapshot id");
        }

        let tp = self.town_paths(name)?;
        let _guard = tp.acquire_lock()?;
        let runtime = self.load_runtime(&tp)?;
        if runtime.state == SessionState::Running || runtime.state == SessionState::Starting {
            bail!("cannot restore while town '{name}' is running");
        }

        let snapshot_dir = tp.snapshots_dir.join(snapshot);
        let meta_path = snapshot_dir.join("snapshot.toml");
        if !meta_path.exists() {
            bail!("snapshot '{snapshot}' not found");
        }
        let meta: SnapshotMeta = toml::from_str(&std::fs::read_to_string(&meta_path)?)?;
        for file in &meta.files {
            let source = snapshot_dir.join("files").join(&file.name);
            if !source.exists() {
                bail!("snapshot is missing file '{}'; refusing restore", file.name);
            }
            let current_hash = storage::sha256_file(&source)?;
            if current_hash != file.sha256 {
                bail!(
                    "snapshot integrity check failed for '{}': expected {}, got {}",
                    file.name,
                    file.sha256,
                    current_hash
                );
            }
            let target = tp.saves_dir.join(&file.name);
            storage::copy_with_backup(&source, &target, &tp.saves_backup_dir)?;
        }
        println!("Restored snapshot '{snapshot}' for town '{name}'");
        Ok(())
    }

    pub fn restock(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let latest = storage::latest_snapshot_dir(&tp.snapshots_dir)
            .with_context(|| format!("town '{name}' has no snapshots to restock from"))?;
        let snapshot = latest
            .file_name()
            .and_then(|s| s.to_str())
            .context("invalid snapshot directory name")?;
        self.restore_snapshot(name, snapshot)
    }

    pub async fn start_town(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let _guard = tp.acquire_lock()?;
        self.enforce_active_limit(name)?;

        let cfg = self.load_town_config(&tp)?;
        let mut runtime = self.load_runtime(&tp)?;
        if runtime.state == SessionState::Running {
            bail!("town '{name}' is already running");
        }

        runtime.state = SessionState::Starting;
        runtime.last_error = None;
        runtime.backend_kind = backend_kind(&cfg.backend).to_string();
        self.save_runtime(&tp, &runtime)?;

        let backend = HostingBackend::from_config(&cfg.backend);
        match backend.start(&tp.logs_dir).await {
            Ok(started) => {
                runtime.state = SessionState::Running;
                runtime.pid = started.pid;
                runtime.started_at = Some(Utc::now());
                runtime.ended_at = None;
                runtime.last_error = None;
                self.save_runtime(&tp, &runtime)?;
                println!("Started town '{name}': {}", started.details);
                println!(
                    "Session limit: {} minutes",
                    cfg.session_limit_minutes
                        .unwrap_or(self.config.default_session_limit_minutes)
                );
            }
            Err(err) => {
                runtime.state = SessionState::Error;
                runtime.last_error = Some(err.to_string());
                runtime.ended_at = Some(Utc::now());
                self.save_runtime(&tp, &runtime)?;
                return Err(err);
            }
        }

        Ok(())
    }

    pub async fn stop_town(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let _guard = tp.acquire_lock()?;
        let cfg = self.load_town_config(&tp)?;
        let mut runtime = self.load_runtime(&tp)?;

        if runtime.state != SessionState::Running && runtime.state != SessionState::Error {
            bail!("town '{name}' is not running");
        }

        runtime.state = SessionState::Stopping;
        self.save_runtime(&tp, &runtime)?;

        let backend = HostingBackend::from_config(&cfg.backend);
        if let Err(err) = backend.stop(runtime.pid).await {
            runtime.state = SessionState::Error;
            runtime.last_error = Some(err.to_string());
            runtime.ended_at = Some(Utc::now());
            self.save_runtime(&tp, &runtime)?;
            return Err(err);
        }

        runtime.state = SessionState::Stopped;
        runtime.pid = None;
        runtime.ended_at = Some(Utc::now());
        runtime.last_error = None;
        self.save_runtime(&tp, &runtime)?;
        println!("Stopped town '{name}'");
        Ok(())
    }

    pub async fn restart_town(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let mut runtime = self.load_runtime(&tp)?;
        runtime.restart_attempts = runtime.restart_attempts.saturating_add(1);
        self.save_runtime(&tp, &runtime)?;

        if runtime.state == SessionState::Running || runtime.state == SessionState::Error {
            self.stop_town(name).await?;
        }
        self.start_town(name).await
    }

    pub fn status(&self, name: Option<&str>, all: bool) -> Result<()> {
        if all {
            for town_name in self.iter_town_names()? {
                self.print_status_line(&town_name)?;
            }
            return Ok(());
        }

        let name = name.context("provide a town name or --all")?;
        self.print_status_line(name)
    }

    pub fn logs(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let stdout_log = tp.logs_dir.join("stdout.log");
        let stderr_log = tp.logs_dir.join("stderr.log");

        if stdout_log.exists() {
            println!("== stdout.log ==");
            print!("{}", std::fs::read_to_string(&stdout_log)?);
        }
        if stderr_log.exists() {
            println!("== stderr.log ==");
            print!("{}", std::fs::read_to_string(&stderr_log)?);
        }

        if !tp.logs_dir.exists() || (!stdout_log.exists() && !stderr_log.exists()) {
            println!("No logs found for town '{name}'");
        }
        Ok(())
    }

    fn print_status_line(&self, name: &str) -> Result<()> {
        let tp = self.town_paths(name)?;
        let mut runtime = self.load_runtime(&tp)?;

        if runtime.state == SessionState::Running
            && let Some(pid) = runtime.pid
            && !process_monitor::is_pid_running(pid)
        {
            runtime.state = SessionState::Error;
            runtime.last_error = Some(format!(
                "process {pid} is no longer running; town may have crashed"
            ));
            runtime.ended_at = Some(Utc::now());
            self.save_runtime(&tp, &runtime)?;
        }

        let duration = runtime
            .started_at
            .map(|start| (Utc::now() - start).num_seconds())
            .unwrap_or(0);
        println!(
            "{name}: state={:?} pid={:?} duration_seconds={} last_error={}",
            runtime.state,
            runtime.pid,
            duration,
            runtime.last_error.as_deref().unwrap_or("none")
        );
        Ok(())
    }

    fn iter_town_names(&self) -> Result<Vec<String>> {
        self.paths.ensure_layout()?;
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.paths.towns_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                names.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    fn enforce_active_limit(&self, target: &str) -> Result<()> {
        let mut active = 0usize;
        for name in self.iter_town_names()? {
            if name == target {
                continue;
            }
            let tp = self.town_paths(&name)?;
            if !tp.runtime_file.exists() {
                continue;
            }
            let runtime = self.load_runtime(&tp)?;
            if runtime.state == SessionState::Running || runtime.state == SessionState::Starting {
                active = active.saturating_add(1);
            }
        }

        if active >= self.config.max_active_towns {
            bail!(
                "active town limit reached ({}). Stop another town or increase max_active_towns in config.toml",
                self.config.max_active_towns
            );
        }

        Ok(())
    }

    fn load_town_config(&self, tp: &TownPaths) -> Result<TownConfig> {
        if !tp.config_file.exists() {
            bail!("town '{}' does not exist", tp.name);
        }
        let raw = std::fs::read_to_string(&tp.config_file)?;
        Ok(toml::from_str(&raw)?)
    }

    fn load_runtime(&self, tp: &TownPaths) -> Result<SessionRuntime> {
        if !tp.runtime_file.exists() {
            return Ok(SessionRuntime::default());
        }
        let raw = std::fs::read_to_string(&tp.runtime_file)?;
        Ok(toml::from_str(&raw)?)
    }

    fn save_runtime(&self, tp: &TownPaths, runtime: &SessionRuntime) -> Result<()> {
        let raw = toml::to_string_pretty(runtime)?;
        storage::atomic_write(&tp.runtime_file, raw.as_bytes())
    }
}

fn backend_kind(backend: &BackendConfig) -> &'static str {
    match backend {
        BackendConfig::Manual(_) => "manual",
        BackendConfig::ExternalProcess(_) => "external_process",
    }
}

#[derive(Debug, Clone)]
struct TownPaths {
    name: String,
    root: PathBuf,
    config_file: PathBuf,
    saves_dir: PathBuf,
    saves_backup_dir: PathBuf,
    snapshots_dir: PathBuf,
    logs_dir: PathBuf,
    runtime_file: PathBuf,
    lock_file: PathBuf,
}

impl TownPaths {
    fn new(towns_root: &Path, name: &str) -> Self {
        let root = towns_root.join(name);
        let runtime_dir = root.join("runtime");
        Self {
            name: name.to_string(),
            config_file: root.join("town.toml"),
            saves_dir: root.join("saves"),
            saves_backup_dir: root.join("saves/backups"),
            snapshots_dir: root.join("snapshots"),
            logs_dir: root.join("logs"),
            runtime_file: runtime_dir.join("session.toml"),
            lock_file: runtime_dir.join("operation.lock"),
            root,
        }
    }

    fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.saves_dir)?;
        std::fs::create_dir_all(&self.snapshots_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        std::fs::create_dir_all(
            self.runtime_file
                .parent()
                .context("runtime file has no parent")?,
        )?;
        Ok(())
    }

    fn acquire_lock(&self) -> Result<LockGuard> {
        self.ensure_dirs()?;
        let file = storage::lock_file(&self.lock_file)?;
        Ok(LockGuard {
            lock_path: self.lock_file.clone(),
            _file: file,
        })
    }
}

struct LockGuard {
    lock_path: PathBuf,
    _file: std::fs::File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        storage::release_lock(&self.lock_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppPaths;
    use crate::hosting::{BackendConfig, ExternalProcessBackendConfig};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn test_manager() -> (TempDir, TownManager) {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("data");
        let paths = AppPaths {
            config_file: root.join("config.toml"),
            towns_dir: root.join("towns"),
            templates_dir: root.join("templates"),
            root,
        };
        paths.ensure_layout().expect("layout");
        let manager = TownManager::new(paths, AppConfig::default());
        (tmp, manager)
    }

    #[test]
    fn create_and_delete_town() {
        let (_tmp, manager) = test_manager();
        manager.create_town("alpha").expect("create");
        let tp = manager.town_paths("alpha").expect("paths");
        assert!(tp.root.exists());

        manager.delete_town("alpha").expect("delete");
        assert!(!tp.root.exists());
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let (_tmp, manager) = test_manager();
        manager.create_town("beta").expect("create");
        let tp = manager.town_paths("beta").expect("paths");
        std::fs::write(tp.saves_dir.join("garden.dat"), b"one").expect("write save");

        manager.backup_town("beta").expect("backup");
        std::fs::write(tp.saves_dir.join("garden.dat"), b"two").expect("mutate save");

        let snapshot = std::fs::read_dir(&tp.snapshots_dir)
            .expect("read snapshots")
            .next()
            .expect("snapshot exists")
            .expect("dir entry")
            .file_name()
            .to_string_lossy()
            .to_string();

        manager
            .restore_snapshot("beta", &snapshot)
            .expect("restore snapshot");
        let content = std::fs::read(tp.saves_dir.join("garden.dat")).expect("read restored");
        assert_eq!(content, b"one");
    }

    #[tokio::test]
    async fn external_process_lifecycle() {
        let (_tmp, manager) = test_manager();
        manager.create_town("gamma").expect("create");

        let tp = manager.town_paths("gamma").expect("paths");
        let mut cfg: TownConfig =
            toml::from_str(&std::fs::read_to_string(&tp.config_file).unwrap()).unwrap();
        cfg.backend = BackendConfig::ExternalProcess(ExternalProcessBackendConfig {
            command: "/bin/sleep".into(),
            args: vec!["20".into()],
            working_dir: None,
            env: HashMap::new(),
        });
        storage::atomic_write(
            &tp.config_file,
            toml::to_string_pretty(&cfg).unwrap().as_bytes(),
        )
        .expect("save town config");

        manager.start_town("gamma").await.expect("start");
        let runtime = manager.load_runtime(&tp).expect("runtime");
        assert_eq!(runtime.state, SessionState::Running);
        assert!(runtime.pid.is_some());

        manager.stop_town("gamma").await.expect("stop");
        let runtime = manager.load_runtime(&tp).expect("runtime2");
        assert_eq!(runtime.state, SessionState::Stopped);
    }
}
