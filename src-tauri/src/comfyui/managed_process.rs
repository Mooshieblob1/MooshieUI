//! Ownership of managed ComfyUI processes, including processes kept alive across app sessions.
//! A listening port or the presence of our custom nodes is never proof of ownership.

use std::collections::HashSet;
use std::net::TcpListener;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::config::AppConfig;
use crate::error::AppError;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProcessIdentity {
    pid: u32,
    started: u64,
    executable: PathBuf,
    #[cfg(windows)]
    windows_created: u64,
}

impl ProcessIdentity {
    fn from_process(process: &sysinfo::Process) -> Option<Self> {
        if process.start_time() == 0 || process.exe()?.as_os_str().is_empty() {
            return None;
        }
        Some(Self {
            pid: process.pid().as_u32(),
            started: process.start_time(),
            executable: process.exe()?.to_path_buf(),
            #[cfg(windows)]
            windows_created: windows_process::created(process.pid().as_u32()).ok()?,
        })
    }

    fn matches(&self, process: &sysinfo::Process) -> bool {
        let matches = self.pid == process.pid().as_u32()
            && self.started != 0
            && self.started == process.start_time()
            && process.exe() == Some(self.executable.as_path());
        #[cfg(windows)]
        return matches && windows_process::created(self.pid).ok() == Some(self.windows_created);
        #[cfg(not(windows))]
        matches
    }

    fn terminate(&self, process: &sysinfo::Process) -> bool {
        #[cfg(windows)]
        {
            let _ = process;
            windows_process::terminate(self.pid, self.windows_created).unwrap_or(false)
        }
        #[cfg(not(windows))]
        process.kill()
    }
}

/// Validate and terminate through the same Windows process handle. This also
/// avoids sysinfo's Windows taskkill subprocess and its PID lookup race.
#[cfg(windows)]
mod windows_process {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE,
    };

    fn open(pid: u32, terminate: bool) -> io::Result<OwnedHandle> {
        let access =
            PROCESS_QUERY_LIMITED_INFORMATION | if terminate { PROCESS_TERMINATE } else { 0 };
        // SAFETY: OpenProcess takes scalar arguments. The returned owned handle
        // is checked for null and closed by OwnedHandle on every return path.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn creation_time(handle: &OwnedHandle) -> io::Result<u64> {
        let mut created = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut exited = created;
        let mut kernel = created;
        let mut user = created;
        // SAFETY: handle is live and has query rights; all output pointers refer
        // to initialized FILETIME values for the duration of the call.
        if unsafe {
            GetProcessTimes(
                handle.as_raw_handle(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    pub fn created(pid: u32) -> io::Result<u64> {
        creation_time(&open(pid, false)?)
    }

    pub fn terminate(pid: u32, expected_creation: u64) -> io::Result<bool> {
        let handle = open(pid, true)?;
        if creation_time(&handle)? != expected_creation {
            return Ok(true); // PID was recycled: leave its new owner alone.
        }
        // SAFETY: this is the same live handle whose exact creation time was
        // just checked, and it was opened with PROCESS_TERMINATE rights.
        Ok(unsafe { TerminateProcess(handle.as_raw_handle(), 1) } != 0)
    }
}

fn process_snapshot(pids: ProcessesToUpdate<'_>) -> System {
    let mut system = System::new();
    system.refresh_processes_specifics(
        pids,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    system
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ManagedProcess {
    identity: ProcessIdentity,
    comfyui_path: PathBuf,
    pub port: u16,
}

fn record_path(config: &AppConfig, worker_id: Option<u32>) -> Option<PathBuf> {
    if config.venv_path.is_empty() {
        return None;
    }
    let name = worker_id.map_or_else(|| "main".to_string(), |id| format!("worker-{id}"));
    Some(PathBuf::from(&config.venv_path).join(format!(".mooshieui-process-{name}.json")))
}

impl ManagedProcess {
    pub fn from_child(pid: u32) -> Result<Self, AppError> {
        let pid = Pid::from_u32(pid);
        let system = process_snapshot(ProcessesToUpdate::Some(&[pid]));
        let identity = system
            .process(pid)
            .and_then(ProcessIdentity::from_process)
            .ok_or_else(|| {
                AppError::ProcessSpawnFailed("Cannot identify the managed ComfyUI process".into())
            })?;
        Ok(Self {
            identity,
            comfyui_path: PathBuf::new(),
            port: 0,
        })
    }

    pub fn capture(config: &AppConfig, pid: u32, port: u16) -> Result<Self, AppError> {
        let mut owned = Self::from_child(pid)?;
        owned.comfyui_path = std::fs::canonicalize(&config.comfyui_path)?;
        owned.port = port;
        Ok(owned)
    }

    pub fn save(&self, config: &AppConfig, worker_id: Option<u32>) -> Result<(), AppError> {
        let path = record_path(config, worker_id).ok_or_else(|| {
            AppError::ProcessSpawnFailed("Missing managed Python environment".into())
        })?;
        std::fs::write(path, serde_json::to_vec(self)?)?;
        Ok(())
    }

    pub fn load(config: &AppConfig, worker_id: Option<u32>) -> Option<Self> {
        let record: Self =
            serde_json::from_slice(&std::fs::read(record_path(config, worker_id)?).ok()?).ok()?;
        if record.comfyui_path != std::fs::canonicalize(&config.comfyui_path).ok()? {
            return None;
        }
        record.is_running().then_some(record)
    }

    pub fn is_running(&self) -> bool {
        let pid = Pid::from_u32(self.identity.pid);
        let system = process_snapshot(ProcessesToUpdate::Some(&[pid]));
        system.process(pid).is_some_and(|process| {
            self.identity.matches(process)
                && !matches!(process.status(), sysinfo::ProcessStatus::Zombie)
        })
    }

    /// Stop only this exact process and its descendants. Python's Windows venv
    /// launcher can own another Python process which actually hosts ComfyUI.
    pub fn stop(&self) -> Result<(), AppError> {
        let system = process_snapshot(ProcessesToUpdate::All);
        let root = Pid::from_u32(self.identity.pid);
        if !system
            .process(root)
            .is_some_and(|process| self.identity.matches(process))
        {
            return Ok(());
        }
        let mut owned = HashSet::from([root]);
        let mut descendants = Vec::new();
        loop {
            let next: Vec<_> = system
                .processes()
                .iter()
                .filter_map(|(pid, process)| {
                    (!owned.contains(pid)
                        && process.start_time() >= self.identity.started
                        && process
                            .parent()
                            .is_some_and(|parent| owned.contains(&parent)))
                    .then(|| ProcessIdentity::from_process(process))
                    .flatten()
                })
                .collect();
            if next.is_empty() {
                break;
            }
            for identity in next {
                owned.insert(Pid::from_u32(identity.pid));
                descendants.push(identity);
            }
        }
        // Recheck each identity immediately before terminating it. Never act on
        // a PID that was recycled since either the saved record or the snapshot.
        descendants.reverse();
        // Stop the launcher first so it cannot respawn a child while we stop
        // the descendants captured above.
        descendants.insert(0, self.identity.clone());
        for identity in descendants {
            let pid = Pid::from_u32(identity.pid);
            let current = process_snapshot(ProcessesToUpdate::Some(&[pid]));
            if let Some(process) = current
                .process(pid)
                .filter(|process| identity.matches(process))
            {
                if !identity.terminate(process) {
                    let recheck = process_snapshot(ProcessesToUpdate::Some(&[pid]));
                    if recheck
                        .process(pid)
                        .is_some_and(|process| identity.matches(process))
                    {
                        return Err(AppError::Other(format!(
                            "Could not stop managed ComfyUI process {}",
                            identity.pid
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Reserve the preferred loopback port, or let the OS allocate a free one.
/// The caller holds this socket until immediately before spawning ComfyUI.
pub(super) fn reserve_port(preferred: u16) -> Result<TcpListener, AppError> {
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, preferred))
        .or_else(|_| TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupied_port_is_left_alone_and_another_port_is_reserved() {
        let external = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = external.local_addr().unwrap().port();
        let managed = reserve_port(port).unwrap();
        assert_ne!(managed.local_addr().unwrap().port(), port);
        assert!(TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_err());
    }

    #[test]
    fn recycled_pid_or_different_executable_does_not_match() {
        let pid = Pid::from_u32(std::process::id());
        let system = process_snapshot(ProcessesToUpdate::Some(&[pid]));
        let process = system.process(pid).unwrap();
        let identity = ProcessIdentity::from_process(process).unwrap();
        assert!(identity.matches(process));
        let mut recycled = identity.clone();
        recycled.started += 1;
        assert!(!recycled.matches(process));
        let mut unrelated = identity;
        unrelated.executable = PathBuf::from("not-the-same-python");
        assert!(!unrelated.matches(process));
    }

    struct TestChild(std::process::Child);

    impl TestChild {
        fn spawn() -> Self {
            #[cfg(windows)]
            let mut command = super::super::process::std_command_no_window("powershell");
            #[cfg(windows)]
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ]);
            #[cfg(not(windows))]
            let mut command = std::process::Command::new("sleep");
            #[cfg(not(windows))]
            command.arg("30");
            Self(command.spawn().unwrap())
        }
    }

    impl Drop for TestChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn keep_alive_record_reclaims_only_the_exact_owned_process() {
        let directory =
            std::env::temp_dir().join(format!("mooshie-ownership-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let config = AppConfig {
            comfyui_path: directory.to_string_lossy().into_owned(),
            venv_path: directory.to_string_lossy().into_owned(),
            ..AppConfig::default()
        };
        let mut owned_child = TestChild::spawn();
        let mut external_child = TestChild::spawn();
        let owned = ManagedProcess::capture(&config, owned_child.0.id(), 18288).unwrap();
        owned.save(&config, None).unwrap();
        assert_eq!(ManagedProcess::load(&config, None).unwrap().port, 18288);

        let mut stale = owned.clone();
        stale.identity.started += 1;
        stale.save(&config, None).unwrap();
        assert!(ManagedProcess::load(&config, None).is_none());
        stale.stop().unwrap();
        assert!(owned_child.0.try_wait().unwrap().is_none());

        #[cfg(windows)]
        {
            let mut recycled = owned.clone();
            recycled.identity.windows_created += 1;
            recycled.save(&config, None).unwrap();
            assert!(ManagedProcess::load(&config, None).is_none());
            recycled.stop().unwrap();
            assert!(owned_child.0.try_wait().unwrap().is_none());
        }

        owned.save(&config, None).unwrap();
        ManagedProcess::load(&config, None).unwrap().stop().unwrap();
        owned_child.0.wait().unwrap();
        assert!(external_child.0.try_wait().unwrap().is_none());
        assert!(ManagedProcess::load(&config, None).is_none());
        std::fs::remove_file(record_path(&config, None).unwrap()).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
