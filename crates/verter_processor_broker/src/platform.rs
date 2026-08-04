#[cfg(target_os = "linux")]
#[path = "platform/linux.rs"]
mod imp;
#[cfg(target_os = "macos")]
#[path = "platform/macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "platform/windows.rs"]
mod imp;

pub(crate) use imp::{
    apply_worker_sandbox, attempt_child_process, random_fill, sandbox_profile_hash,
    spawn_denied_worker, wait_readable, worker_stream_from_args, PlatformChild, PlatformStream,
};
#[cfg(target_os = "linux")]
pub(crate) use imp::{attempt_direct_open, attempt_openat2};

pub(crate) struct SpawnedWorker {
    pub child: PlatformChild,
    pub stream: PlatformStream,
    pub executable: std::path::PathBuf,
}

#[cfg(test)]
pub(crate) use imp::wait_pid_gone_for_test;
