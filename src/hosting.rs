use crate::process_monitor;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendConfig {
    Manual(ManualBackendConfig),
    ExternalProcess(ExternalProcessBackendConfig),
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::Manual(ManualBackendConfig {})
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManualBackendConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalProcessBackendConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SessionStart {
    pub pid: Option<u32>,
    pub details: String,
}

pub enum HostingBackend {
    Manual(ManualBackend),
    ExternalProcess(ExternalProcessBackend),
}

impl HostingBackend {
    pub fn from_config(config: &BackendConfig) -> Self {
        match config {
            BackendConfig::Manual(_) => Self::Manual(ManualBackend {}),
            BackendConfig::ExternalProcess(cfg) => Self::ExternalProcess(ExternalProcessBackend {
                config: cfg.clone(),
            }),
        }
    }

    pub async fn start(&self, log_dir: &Path) -> Result<SessionStart> {
        match self {
            Self::Manual(backend) => backend.start().await,
            Self::ExternalProcess(backend) => backend.start(log_dir).await,
        }
    }

    pub async fn stop(&self, pid: Option<u32>) -> Result<()> {
        match self {
            Self::Manual(backend) => backend.stop().await,
            Self::ExternalProcess(backend) => backend.stop(pid).await,
        }
    }
}

pub struct ManualBackend {}

impl ManualBackend {
    pub async fn start(&self) -> Result<SessionStart> {
        Ok(SessionStart {
            pid: None,
            details: "Manual backend started. Manage the physical 3DS session yourself.".into(),
        })
    }

    pub async fn stop(&self) -> Result<()> {
        Ok(())
    }
}

pub struct ExternalProcessBackend {
    config: ExternalProcessBackendConfig,
}

impl ExternalProcessBackend {
    pub async fn start(&self, log_dir: &Path) -> Result<SessionStart> {
        if self.config.command.trim().is_empty() {
            bail!("external_process backend command cannot be empty");
        }

        std::fs::create_dir_all(log_dir)?;
        let stdout_path = log_dir.join("stdout.log");
        let stderr_path = log_dir.join("stderr.log");
        let stdout = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stdout_path)?;
        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stderr_path)?;

        let mut cmd = tokio::process::Command::new(&self.config.command);
        cmd.args(&self.config.args)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .stdin(Stdio::null())
            .kill_on_drop(false);

        if let Some(dir) = &self.config.working_dir {
            cmd.current_dir(dir);
        }

        if !self.config.env.is_empty() {
            cmd.envs(&self.config.env);
        }

        let child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn external process command '{}'",
                self.config.command
            )
        })?;

        let pid = child
            .id()
            .with_context(|| "failed to read process id from spawned command")?;

        Ok(SessionStart {
            pid: Some(pid),
            details: format!("External process started with pid {pid}"),
        })
    }

    pub async fn stop(&self, pid: Option<u32>) -> Result<()> {
        let pid = pid.context("no pid recorded for external process backend")?;
        process_monitor::assert_pid_running(pid)?;
        process_monitor::terminate_pid(pid)?;
        Ok(())
    }
}
