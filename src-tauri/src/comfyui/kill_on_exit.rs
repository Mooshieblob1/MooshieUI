//! Windows: tie managed ComfyUI processes to MooshieUI's lifetime.
//!
//! `kill_on_drop` and the shutdown hooks only run when MooshieUI exits
//! normally. A crash, or "End task" in Task Manager, skips them and leaves the
//! venv launcher and the ComfyUI it hosts running, still holding VRAM and still
//! working through whatever job it had. Every child added here belongs to a job
//! object whose only handle MooshieUI holds, so Windows terminates them when
//! that handle closes, however MooshieUI exits. Keep-alive spawns stay out.
//!
//! Only the venv launcher has to be assigned. It runs the real ComfyUI python
//! inside its own kill-on-close job, so taking the launcher down takes ComfyUI
//! with it, even if the launcher spawned that child before being assigned here.

use std::os::windows::io::RawHandle;
use std::sync::OnceLock;

use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// The job handle, as an integer so it can live in a static. Never closed:
/// Windows closes it when MooshieUI exits, which is the point.
static JOB: OnceLock<Option<usize>> = OnceLock::new();

fn job() -> Option<usize> {
    *JOB.get_or_init(|| {
        // SAFETY: null attributes and name are documented as valid and give an
        // unnamed job with default security.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            log::warn!(
                "Could not create the ComfyUI job object: {}",
                std::io::Error::last_os_error()
            );
            return None;
        }
        // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is a valid
        // "no limits" value; only the kill-on-close flag is set on top.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `job` is the live handle created above; `info` outlives the
        // call and its size is passed alongside it.
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            log::warn!(
                "Could not make the ComfyUI job object kill on close: {}",
                std::io::Error::last_os_error()
            );
            // Leak the handle rather than close it: an unconfigured job is
            // harmless, and nothing is ever assigned to it.
            return None;
        }
        Some(job as usize)
    })
}

/// Put a freshly spawned managed process in the kill-on-exit job.
/// Best effort: a failure is logged and the process keeps running untied.
pub fn assign(process: RawHandle) {
    let Some(job) = job() else {
        return;
    };
    // SAFETY: `job` is the live job handle (never closed), and `process` is the
    // spawned child's own handle, which has full access rights and stays open
    // for as long as the caller holds the child.
    if unsafe { AssignProcessToJobObject(job as _, process as _) } == 0 {
        log::warn!(
            "Could not tie ComfyUI to MooshieUI's lifetime; it may outlive a crash: {}",
            std::io::Error::last_os_error()
        );
    }
}
