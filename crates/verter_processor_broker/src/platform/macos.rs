use std::ffi::{c_char, c_int, c_void, CString, OsStr};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::attestation::domain_hash;
use crate::channel::ChannelError;
use crate::lifecycle::{BrokerError, SandboxUnavailableEvidence};
use crate::platform::SpawnedWorker;
use crate::policy::ProcessorSandboxKindV1;

const SEATBELT_PROFILE: &str = "(version 1)(deny default)";

#[link(name = "sandbox")]
unsafe extern "C" {
    fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
    fn sandbox_free_error(errorbuf: *mut c_char);
}

unsafe extern "C" {
    fn arc4random_buf(buffer: *mut c_void, length: usize);
}

pub(crate) type PlatformStream = UnixStream;

pub(crate) struct PlatformChild {
    child: Child,
}

impl PlatformChild {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn kill_tree(&mut self) {
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.kill();
    }

    pub fn wait_bounded(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return status.code(),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                _ => return None,
            }
        }
    }

    fn has_exited(&mut self) -> Option<i32> {
        self.child
            .try_wait()
            .ok()
            .flatten()
            .and_then(|status| status.code())
    }
}

impl Drop for PlatformChild {
    fn drop(&mut self) {
        self.kill_tree();
        self.wait_bounded(Duration::from_secs(5));
    }
}

pub(crate) fn random_fill(bytes: &mut [u8]) -> Result<(), BrokerError> {
    unsafe { arc4random_buf(bytes.as_mut_ptr().cast(), bytes.len()) };
    Ok(())
}

pub(crate) fn sandbox_profile_hash() -> [u8; 32] {
    domain_hash(
        b"macos-seatbelt-profile\0",
        &[
            SEATBELT_PROFILE.as_bytes(),
            b"explicit-inherited-fd",
            b"empty-environment",
        ],
    )
}

pub(crate) fn spawn_denied_worker(
    executable: &Path,
    _launch_nonce: &[u8; 16],
) -> Result<SpawnedWorker, BrokerError> {
    let (broker_stream, worker_stream) = UnixStream::pair()?;
    let worker_fd = worker_stream.as_raw_fd();
    let mut command = Command::new(executable);
    command
        .arg("--broker-fd")
        .arg(worker_fd.to_string())
        .arg("--worker-executable")
        .arg(executable)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            let max_fd = libc::getdtablesize();
            for fd in 0..max_fd {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
            if libc::fcntl(worker_fd, libc::F_SETFD, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().map_err(|error| {
        BrokerError::SandboxUnavailable(SandboxUnavailableEvidence::new(
            ProcessorSandboxKindV1::MacSandbox,
            "explicit-fd worker launch",
            error.raw_os_error(),
        ))
    })?;
    drop(worker_stream);
    Ok(SpawnedWorker {
        child: PlatformChild { child },
        stream: broker_stream,
        executable: executable.to_path_buf(),
    })
}

pub(crate) fn read_some_by_deadline(
    stream: &mut PlatformStream,
    mut child: Option<&mut PlatformChild>,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, ChannelError> {
    use std::io::Read;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut descriptor = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = i32::try_from(remaining.as_millis().min(10)).unwrap_or(10);
        if unsafe { libc::poll(&mut descriptor, 1, millis) } > 0
            && descriptor.revents & libc::POLLIN != 0
        {
            let read = stream.read(buffer)?;
            if read == 0 {
                return Err(ChannelError::Io("worker stream reached end of file".into()));
            }
            return Ok(read);
        }
        if let Some(child) = child.as_deref_mut() {
            if let Some(status) = child.has_exited() {
                return Err(ChannelError::Io(format!(
                    "worker exited with status {status}"
                )));
            }
        }
        if remaining.is_zero() {
            return Err(ChannelError::ReadDeadlineExceeded);
        }
    }
}

pub(crate) fn worker_stream_from_args() -> Result<(PlatformStream, PathBuf), BrokerError> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new("--broker-fd")) {
        return Err(BrokerError::Protocol("missing broker fd"));
    }
    let fd: RawFd = args
        .next()
        .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
        .ok_or(BrokerError::Protocol("invalid broker fd"))?;
    if args.next().as_deref() != Some(OsStr::new("--worker-executable")) {
        return Err(BrokerError::Protocol("missing worker executable"));
    }
    let executable = args
        .next()
        .map(PathBuf::from)
        .ok_or(BrokerError::Protocol("invalid worker executable"))?;
    Ok((unsafe { UnixStream::from_raw_fd(fd) }, executable))
}

pub(crate) fn apply_worker_sandbox() -> Result<(), BrokerError> {
    let profile = CString::new(SEATBELT_PROFILE).expect("static profile has no NUL");
    let mut error = std::ptr::null_mut();
    if unsafe { sandbox_init(profile.as_ptr(), 0, &mut error) } != 0 {
        if !error.is_null() {
            unsafe { sandbox_free_error(error) };
        }
        return Err(BrokerError::SandboxUnavailable(
            SandboxUnavailableEvidence::new(
                ProcessorSandboxKindV1::MacSandbox,
                "sandbox_init deny-default profile",
                None,
            ),
        ));
    }
    Ok(())
}

pub(crate) fn attempt_child_process() -> bool {
    Command::new("/usr/bin/true")
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
pub(crate) fn wait_pid_gone_for_test(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if unsafe { libc::kill(pid as i32, 0) } != 0
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}
