use anyhow::{Result, bail};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

pub fn is_pid_running(pid: u32) -> bool {
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

pub fn terminate_pid(pid: u32) -> Result<()> {
    let proc = Pid::from_raw(pid as i32);
    kill(proc, Signal::SIGTERM).map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

pub fn assert_pid_running(pid: u32) -> Result<()> {
    if is_pid_running(pid) {
        Ok(())
    } else {
        bail!("process with pid {pid} is not running")
    }
}
