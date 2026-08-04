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
use crate::channel::ChannelError;
use crate::lifecycle::{BrokerError, SandboxUnavailableEvidence};
use crate::platform::SpawnedWorker;
use crate::policy::ProcessorSandboxKindV1;

/// tmpfs mount data for the private empty worker root.
const ROOT_TMPFS_DATA: &str = "size=32m,mode=0755\0";
/// `PR_SET_NO_NEW_PRIVS` value applied at both sandbox stages.
const NO_NEW_PRIVILEGES_VALUE: i32 = 1;
/// First file descriptor the `close_range` sweep retains.
const CLOSE_RANGE_FIRST_RETAINED_FD: u32 = 4;

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

/// The concrete namespace/seccomp configuration the launch path actually enforces.
///
/// Every field is consumed by an enforcement site in this module, so the profile
/// hash changes iff the enforced policy changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinuxSandboxPolicyMaterial {
    /// `unshare` flags applied in `pre_exec`.
    pub(crate) unshare_flags: i32,
    /// `mount` flags for the private empty root tmpfs.
    pub(crate) root_mount_flags: u64,
    /// tmpfs mount data for that root.
    pub(crate) root_mount_data: &'static str,
    /// Remount flags applied to every read-only bind mount.
    pub(crate) bind_remount_flags: u64,
    /// `PR_SET_NO_NEW_PRIVS` value.
    pub(crate) no_new_privileges: i32,
    /// Whether supplementary groups are denied before the uid/gid maps are written.
    pub(crate) setgroups_denied: bool,
    /// First fd retained by the `close_range` sweep.
    pub(crate) close_range_first_retained_fd: u32,
    /// The launch-stage seccomp filter program bytes.
    pub(crate) launch_filter: Vec<u8>,
    /// The worker-stage seccomp filter program bytes.
    pub(crate) worker_filter: Vec<u8>,
    /// The audit arch the filter pins.
    pub(crate) audit_arch: u32,
}

pub(crate) fn enforced_linux_sandbox_policy() -> LinuxSandboxPolicyMaterial {
    LinuxSandboxPolicyMaterial {
        unshare_flags: libc::CLONE_NEWUSER | libc::CLONE_NEWNS | libc::CLONE_NEWNET,
        root_mount_flags: libc::MS_NOSUID | libc::MS_NODEV,
        root_mount_data: ROOT_TMPFS_DATA,
        bind_remount_flags: libc::MS_BIND
            | libc::MS_REMOUNT
            | libc::MS_RDONLY
            | libc::MS_NOSUID
            | libc::MS_NODEV,
        no_new_privileges: NO_NEW_PRIVILEGES_VALUE,
        setgroups_denied: true,
        close_range_first_retained_fd: CLOSE_RANGE_FIRST_RETAINED_FD,
        launch_filter: encode_seccomp_filter(SeccompStage::Launch),
        worker_filter: encode_seccomp_filter(SeccompStage::Worker),
        audit_arch: native_audit_arch(),
    }
}

fn encode_seccomp_filter(stage: SeccompStage) -> Vec<u8> {
    let filters = seccomp_program(stage);
    let mut out = Vec::with_capacity(filters.len() * 8);
    for filter in filters {
        out.extend_from_slice(&filter.code.to_be_bytes());
        out.push(filter.jt);
        out.push(filter.jf);
        out.extend_from_slice(&filter.k.to_be_bytes());
    }
    out
}

pub(crate) fn hash_linux_sandbox_policy(material: &LinuxSandboxPolicyMaterial) -> [u8; 32] {
    let mut encoded =
        Vec::with_capacity(material.launch_filter.len() + material.worker_filter.len() + 64);
    encoded.extend_from_slice(&material.unshare_flags.to_be_bytes());
    encoded.extend_from_slice(&material.root_mount_flags.to_be_bytes());
    encoded.extend_from_slice(&(material.root_mount_data.len() as u32).to_be_bytes());
    encoded.extend_from_slice(material.root_mount_data.as_bytes());
    encoded.extend_from_slice(&material.bind_remount_flags.to_be_bytes());
    encoded.extend_from_slice(&material.no_new_privileges.to_be_bytes());
    encoded.push(u8::from(material.setgroups_denied));
    encoded.extend_from_slice(&material.close_range_first_retained_fd.to_be_bytes());
    encoded.extend_from_slice(&(material.launch_filter.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&material.launch_filter);
    encoded.extend_from_slice(&(material.worker_filter.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&material.worker_filter);
    encoded.extend_from_slice(&material.audit_arch.to_be_bytes());
    domain_hash(b"linux-namespace-seccomp-enforced-policy\0", &[&encoded])
}

/// Digests the namespace and seccomp configuration this module actually installs, so
/// the attested profile hash changes if and only if the enforced policy changes.
pub(crate) fn sandbox_profile_hash() -> [u8; 32] {
    hash_linux_sandbox_policy(&enforced_linux_sandbox_policy())
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
        let result = unsafe { libc::poll(&mut descriptor, 1, millis) };
        if result > 0 && descriptor.revents & libc::POLLIN != 0 {
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
    if libc::unshare(enforced_linux_sandbox_policy().unshare_flags) != 0 {
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
        enforced_linux_sandbox_policy().root_mount_flags as libc::c_ulong,
        cstr(ROOT_TMPFS_DATA.as_bytes()).cast(),
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
    let close_result = libc::syscall(
        libc::SYS_close_range,
        0_u32,
        u32::MAX,
        CLOSE_RANGE_FIRST_RETAINED_FD,
    );
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
    let remount = if recursive {
        enforced_linux_sandbox_policy().bind_remount_flags as libc::c_ulong | libc::MS_REC
    } else {
        enforced_linux_sandbox_policy().bind_remount_flags as libc::c_ulong
    };
    if libc::mount(
        std::ptr::null(),
        target,
        std::ptr::null(),
        remount,
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
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, NO_NEW_PRIVILEGES_VALUE, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn seccomp_program(stage: SeccompStage) -> Vec<libc::sock_filter> {
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
    filters
}

fn install_seccomp(stage: SeccompStage) -> io::Result<()> {
    let mut filters = seccomp_program(stage);
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
