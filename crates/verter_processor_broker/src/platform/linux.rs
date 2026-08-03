use std::ffi::{CString, OsStr};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::attestation::domain_hash;
use crate::lifecycle::{BrokerError, SandboxUnavailableEvidence};
use crate::platform::SpawnedWorker;
use crate::policy::ProcessorSandboxKindV1;

pub(crate) type PlatformStream = UnixStream;

pub(crate) struct PlatformChild {
    child: Child,
    root: PathBuf,
}

impl PlatformChild {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn kill_tree(&mut self) {
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
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
        let _ = std::fs::remove_dir(&self.root);
    }
}

pub(crate) fn random_fill(bytes: &mut [u8]) -> Result<(), BrokerError> {
    let mut written = 0_usize;
    while written < bytes.len() {
        let result = unsafe {
            libc::syscall(
                libc::SYS_getrandom,
                bytes[written..].as_mut_ptr(),
                bytes.len() - written,
                0,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        written += result as usize;
    }
    Ok(())
}

pub(crate) fn sandbox_profile_hash() -> [u8; 32] {
    domain_hash(
        b"linux-namespace-seccomp-profile\0",
        &[
            b"user+mount+network-namespaces",
            b"private-empty-root",
            b"no-new-privileges",
            b"seccomp-deny-file+network+process+escape",
            b"closed-fds-except-broker-ipc",
            b"empty-environment",
        ],
    )
}

pub(crate) fn spawn_denied_worker(
    source_executable: &Path,
    launch_nonce: &[u8; 16],
) -> Result<SpawnedWorker, BrokerError> {
    let (broker_stream, worker_stream) = UnixStream::pair()?;
    let worker_fd = worker_stream.as_raw_fd();
    let root = std::env::temp_dir().join(format!("verter-worker-{}", hex(launch_nonce)));
    std::fs::create_dir(&root)?;
    let root_for_child = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| BrokerError::Protocol("sandbox root contains NUL"))?;
    let executable_for_child = CString::new(source_executable.as_os_str().as_bytes())
        .map_err(|_| BrokerError::Protocol("worker path contains NUL"))?;
    let inherited_uid = unsafe { libc::geteuid() };
    let inherited_gid = unsafe { libc::getegid() };
    let mut command = Command::new("/worker");
    command
        .arg("--broker-fd")
        .arg(worker_fd.to_string())
        .arg("--worker-executable")
        .arg("/worker")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(move || {
            setup_namespaces_and_root(
                &root_for_child,
                &executable_for_child,
                inherited_uid,
                inherited_gid,
                worker_fd,
            )
        });
    }
    let child = command.spawn().map_err(|error| {
        let _ = std::fs::remove_dir(&root);
        unavailable("namespace/mount/seccomp launch", error.raw_os_error())
    })?;
    drop(worker_stream);
    Ok(SpawnedWorker {
        child: PlatformChild {
            child,
            root: root.clone(),
        },
        stream: broker_stream,
        executable: source_executable.to_path_buf(),
    })
}

pub(crate) fn wait_readable(
    stream: &mut PlatformStream,
    child: &mut PlatformChild,
    timeout: Duration,
) -> Result<(), BrokerError> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(BrokerError::WorkerTimeout);
        }
        let mut descriptor = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = i32::try_from(remaining.as_millis().min(10)).unwrap_or(10);
        let result = unsafe { libc::poll(&mut descriptor, 1, millis) };
        if result > 0 && descriptor.revents & libc::POLLIN != 0 {
            return Ok(());
        }
        if let Some(status) = child.has_exited() {
            return Err(BrokerError::WorkerCrashed(Some(status)));
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
    set_no_new_privileges()
        .map_err(|error| unavailable("PR_SET_NO_NEW_PRIVS(worker)", error.raw_os_error()))?;
    install_seccomp(SeccompStage::Worker)
        .map_err(|error| unavailable("seccomp filter(worker)", error.raw_os_error()))
}

pub(crate) fn attempt_child_process() -> bool {
    Command::new("/worker")
        .arg("--invalid")
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) fn attempt_direct_open() -> bool {
    let result = unsafe {
        libc::syscall(
            libc::SYS_open,
            cstr(b"/worker\0"),
            libc::O_RDONLY | libc::O_CLOEXEC,
        )
    };
    syscall_was_not_denied(result)
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub(crate) const fn attempt_direct_open() -> bool {
    false
}

pub(crate) fn attempt_openat2() -> bool {
    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }

    let how = OpenHow {
        flags: libc::O_RDONLY as u64 | libc::O_CLOEXEC as u64,
        mode: 0,
        resolve: 0,
    };
    let result = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::AT_FDCWD,
            cstr(b"/worker\0"),
            &raw const how,
            std::mem::size_of::<OpenHow>(),
        )
    };
    syscall_was_not_denied(result)
}

fn syscall_was_not_denied(result: libc::c_long) -> bool {
    if result < 0 {
        last_errno() != Some(libc::EPERM)
    } else {
        unsafe { libc::close(result as RawFd) };
        true
    }
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

unsafe fn setup_namespaces_and_root(
    root: &CString,
    executable: &CString,
    uid: libc::uid_t,
    gid: libc::gid_t,
    worker_fd: RawFd,
) -> io::Result<()> {
    if libc::setsid() < 0 {
        return Err(io::Error::last_os_error());
    }
    if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS | libc::CLONE_NEWNET) != 0 {
        return Err(io::Error::last_os_error());
    }
    write_proc(b"/proc/self/setgroups\0", b"deny")?;
    let uid_map = format!("0 {uid} 1");
    write_proc(b"/proc/self/uid_map\0", uid_map.as_bytes())?;
    let gid_map = format!("0 {gid} 1");
    write_proc(b"/proc/self/gid_map\0", gid_map.as_bytes())?;
    if libc::setresgid(0, 0, 0) != 0 || libc::setresuid(0, 0, 0) != 0 {
        return Err(io::Error::last_os_error());
    }
    if libc::mount(
        std::ptr::null(),
        cstr(b"/\0"),
        std::ptr::null(),
        libc::MS_REC | libc::MS_PRIVATE,
        std::ptr::null(),
    ) != 0
    {
        return Err(io::Error::last_os_error());
    }
    if libc::mount(
        cstr(b"tmpfs\0"),
        root.as_ptr(),
        cstr(b"tmpfs\0"),
        libc::MS_NOSUID | libc::MS_NODEV,
        cstr(b"size=32m,mode=0755\0").cast(),
    ) != 0
    {
        return Err(io::Error::last_os_error());
    }
    create_under(root, b"worker\0", false)?;
    bind_mount(
        executable.as_ptr(),
        joined(root, b"worker\0")?.as_ptr(),
        false,
    )?;
    for source in [b"/lib\0".as_slice(), b"/lib64\0", b"/usr/lib\0"] {
        if libc::access(cstr(source), libc::F_OK) == 0 {
            let relative = &source[1..];
            create_under(root, relative, true)?;
            bind_mount(cstr(source), joined(root, relative)?.as_ptr(), true)?;
        }
    }
    if libc::chroot(root.as_ptr()) != 0 || libc::chdir(cstr(b"/\0")) != 0 {
        return Err(io::Error::last_os_error());
    }
    let close_result = libc::syscall(libc::SYS_close_range, 0_u32, u32::MAX, 4_u32);
    if close_result != 0 && last_errno() != Some(libc::ENOSYS) {
        return Err(io::Error::last_os_error());
    }
    if close_result != 0 {
        for fd in 0..1024 {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    if libc::fcntl(worker_fd, libc::F_SETFD, 0) != 0 {
        return Err(io::Error::last_os_error());
    }
    set_no_new_privileges()?;
    install_seccomp(SeccompStage::Launch)?;
    Ok(())
}

unsafe fn write_proc(path: &[u8], bytes: &[u8]) -> io::Result<()> {
    let fd = libc::open(cstr(path), libc::O_WRONLY | libc::O_CLOEXEC);
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let result = libc::write(fd, bytes.as_ptr().cast(), bytes.len());
    let saved = io::Error::last_os_error();
    libc::close(fd);
    if result == bytes.len() as isize {
        Ok(())
    } else {
        Err(saved)
    }
}

unsafe fn create_under(root: &CString, relative: &[u8], directory: bool) -> io::Result<()> {
    let path = joined(root, relative)?;
    if directory {
        if libc::mkdir(path.as_ptr(), 0o755) != 0 && last_errno() != Some(libc::EEXIST) {
            return Err(io::Error::last_os_error());
        }
    } else {
        let fd = libc::open(path.as_ptr(), libc::O_CREAT | libc::O_RDONLY, 0o555);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        libc::close(fd);
    }
    Ok(())
}

unsafe fn bind_mount(source: *const i8, target: *const i8, recursive: bool) -> io::Result<()> {
    let flags = libc::MS_BIND | if recursive { libc::MS_REC } else { 0 };
    if libc::mount(source, target, std::ptr::null(), flags, std::ptr::null()) != 0 {
        return Err(io::Error::last_os_error());
    }
    if libc::mount(
        std::ptr::null(),
        target,
        std::ptr::null(),
        flags | libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV,
        std::ptr::null(),
    ) != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn joined(root: &CString, relative: &[u8]) -> io::Result<CString> {
    let mut bytes = root.as_bytes().to_vec();
    bytes.push(b'/');
    bytes.extend_from_slice(&relative[..relative.len() - 1]);
    CString::new(bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SeccompStage {
    Launch,
    Worker,
}

fn set_no_new_privileges() -> io::Result<()> {
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn install_seccomp(stage: SeccompStage) -> io::Result<()> {
    const LOAD_ARCH: libc::sock_filter = stmt(0x20, 4);
    const LOAD_SYSCALL: libc::sock_filter = stmt(0x20, 0);
    const ALLOW: libc::sock_filter = stmt(0x06, libc::SECCOMP_RET_ALLOW);
    const DENY: libc::sock_filter = stmt(
        0x06,
        libc::SECCOMP_RET_ERRNO | (libc::EPERM as u32 & libc::SECCOMP_RET_DATA),
    );
    const KILL: libc::sock_filter = stmt(0x06, libc::SECCOMP_RET_KILL_PROCESS);
    let denied = denied_syscalls(stage);
    let mut filters = Vec::with_capacity(5 + denied.len() * 2);
    filters.push(LOAD_ARCH);
    filters.push(jump(0x15, native_audit_arch(), 1, 0));
    filters.push(KILL);
    filters.push(LOAD_SYSCALL);
    for syscall in denied {
        filters.push(jump(0x15, syscall as u32, 0, 1));
        filters.push(DENY);
    }
    filters.push(ALLOW);
    let program = libc::sock_fprog {
        len: u16::try_from(filters.len())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
        filter: filters.as_mut_ptr(),
    };
    if unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &raw const program,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn denied_syscalls(stage: SeccompStage) -> Vec<libc::c_long> {
    let mut denied = vec![
        libc::SYS_openat2,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
        libc::SYS_memfd_create,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_recvfrom,
        libc::SYS_sendmsg,
        libc::SYS_recvmsg,
        libc::SYS_sendmmsg,
        libc::SYS_recvmmsg,
        libc::SYS_shutdown,
        libc::SYS_getsockname,
        libc::SYS_getpeername,
        libc::SYS_getsockopt,
        libc::SYS_setsockopt,
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_execveat,
        libc::SYS_pidfd_open,
        libc::SYS_pidfd_getfd,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_kcmp,
        libc::SYS_ptrace,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_move_mount,
        libc::SYS_open_tree,
        libc::SYS_fspick,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_mount_setattr,
        libc::SYS_chroot,
        libc::SYS_setns,
        libc::SYS_unshare,
        libc::SYS_bpf,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_perf_event_open,
        libc::SYS_userfaultfd,
        libc::SYS_fanotify_init,
        libc::SYS_kexec_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_reboot,
        libc::SYS_acct,
        libc::SYS_swapon,
        libc::SYS_swapoff,
    ];
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    denied.extend([
        libc::SYS_open,
        libc::SYS_creat,
        libc::SYS_fork,
        libc::SYS_vfork,
    ]);
    if stage == SeccompStage::Worker {
        denied.extend([libc::SYS_openat, libc::SYS_execve]);
    }
    denied
}

#[cfg(target_arch = "x86_64")]
const fn native_audit_arch() -> u32 {
    0xc000_003e
}

#[cfg(target_arch = "x86")]
const fn native_audit_arch() -> u32 {
    0x4000_0003
}

#[cfg(target_arch = "aarch64")]
const fn native_audit_arch() -> u32 {
    0xc000_00b7
}

#[cfg(target_arch = "arm")]
const fn native_audit_arch() -> u32 {
    0x4000_0028
}

#[cfg(target_arch = "riscv64")]
const fn native_audit_arch() -> u32 {
    0xc000_00f3
}

const fn stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

const fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

fn unavailable(operation: &'static str, os_error: Option<i32>) -> BrokerError {
    BrokerError::SandboxUnavailable(SandboxUnavailableEvidence::new(
        ProcessorSandboxKindV1::LinuxNamespaceSeccomp,
        operation,
        os_error,
    ))
}

fn last_errno() -> Option<i32> {
    io::Error::last_os_error().raw_os_error()
}

const fn cstr(bytes: &[u8]) -> *const i8 {
    bytes.as_ptr().cast()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}
