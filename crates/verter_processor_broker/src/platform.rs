#[cfg(target_os = "linux")]
#[path = "platform/linux.rs"]
mod imp;
#[cfg(target_os = "macos")]
#[path = "platform/macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "platform/windows.rs"]
mod imp;

use std::time::Instant;

use crate::channel::ChannelError;

pub(crate) use imp::{
    apply_worker_sandbox, attempt_child_process, random_fill, sandbox_profile_hash,
    spawn_denied_worker, worker_stream_from_args, PlatformChild, PlatformStream,
};
#[cfg(target_os = "linux")]
pub(crate) use imp::{attempt_direct_open, attempt_openat2};

pub(crate) struct SpawnedWorker {
    pub child: PlatformChild,
    pub stream: PlatformStream,
    pub executable: std::path::PathBuf,
}

/// A worker stream whose every blocking read is bounded by a caller-supplied deadline.
///
/// A worker that announces a frame length and then stalls can no longer park the
/// reading thread: the read expires as `ChannelError::ReadDeadlineExceeded`, which the
/// broker turns into a typed timeout plus worker teardown.
pub(crate) struct DeadlineStream<'a> {
    stream: &'a mut PlatformStream,
    child: Option<&'a mut PlatformChild>,
}

impl<'a> DeadlineStream<'a> {
    pub(crate) fn new(
        stream: &'a mut PlatformStream,
        child: Option<&'a mut PlatformChild>,
    ) -> Self {
        Self { stream, child }
    }

    pub(crate) fn writer(&mut self) -> &mut PlatformStream {
        self.stream
    }

    pub(crate) fn read_exact_by_deadline(
        &mut self,
        buffer: &mut [u8],
        deadline: Instant,
    ) -> Result<(), ChannelError> {
        let mut filled = 0_usize;
        while filled < buffer.len() {
            let read = imp::read_some_by_deadline(
                self.stream,
                self.child.as_deref_mut(),
                &mut buffer[filled..],
                deadline,
            )?;
            filled += read;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) use imp::wait_pid_gone_for_test;
#[cfg(all(test, windows))]
pub(crate) use imp::{
    applied_policy::take_applied_for_test as take_applied_app_container_policy_for_test,
    enforced_app_container_policy, hash_app_container_policy, with_app_container_policy_for_test,
    AppContainerPolicyMaterial, ENFORCED_APP_CONTAINER_POLICY,
};
#[cfg(all(test, target_os = "linux"))]
pub(crate) use imp::{
    count_launch_syscall_denials_for_test, deny_launch_syscall_for_test,
    enforced_linux_sandbox_policy, hash_linux_sandbox_policy, with_linux_sandbox_policy_for_test,
};
