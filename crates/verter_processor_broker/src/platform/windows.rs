use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile,
};
use windows_sys::Win32::Security::{
    FreeSid, GetTokenInformation, TokenIsAppContainer, CONTAINER_INHERIT_ACE,
    DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE, SECURITY_ATTRIBUTES, TOKEN_ALL_ACCESS,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, PeekNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, OpenProcessToken, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY, STARTUPINFOEXW,
};
use windows_sys::Win32::System::WindowsProgramming::{
    PROCESS_CREATION_MITIGATION_POLICY_DEP_ENABLE, PROCESS_CREATION_MITIGATION_POLICY_SEHOP_ENABLE,
};

use crate::attestation::domain_hash;
use crate::lifecycle::{BrokerError, SandboxUnavailableEvidence};
use crate::platform::SpawnedWorker;
use crate::policy::ProcessorSandboxKindV1;

pub(crate) type PlatformStream = File;

pub(crate) struct PlatformChild {
    process: HANDLE,
    job: HANDLE,
    profile_name: Vec<u16>,
    executable: PathBuf,
}

unsafe impl Send for PlatformChild {}

impl PlatformChild {
    pub fn pid(&self) -> u32 {
        unsafe { windows_sys::Win32::System::Threading::GetProcessId(self.process) }
    }

    pub fn kill_tree(&mut self) {
        unsafe {
            TerminateJobObject(self.job, 1);
        }
    }

    pub fn wait_bounded(&mut self, timeout: Duration) -> Option<i32> {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        if unsafe { WaitForSingleObject(self.process, millis) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0_u32;
        if unsafe { GetExitCodeProcess(self.process, &mut code) } == 0 {
            None
        } else {
            Some(code as i32)
        }
    }

    fn has_exited(&mut self) -> Option<i32> {
        if unsafe { WaitForSingleObject(self.process, 0) } == WAIT_OBJECT_0 {
            self.wait_bounded(Duration::ZERO)
        } else {
            None
        }
    }
}

impl Drop for PlatformChild {
    fn drop(&mut self) {
        self.kill_tree();
        self.wait_bounded(Duration::from_secs(5));
        unsafe {
            CloseHandle(self.process);
            CloseHandle(self.job);
        }
        let _ = std::fs::remove_file(&self.executable);
        if let Some(parent) = self.executable.parent() {
            let _ = std::fs::remove_dir(parent);
        }
        unsafe {
            DeleteAppContainerProfile(self.profile_name.as_ptr());
        }
    }
}

pub(crate) fn random_fill(bytes: &mut [u8]) -> Result<(), BrokerError> {
    let length = u32::try_from(bytes.len()).map_err(|_| BrokerError::Protocol("random length"))?;
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        Err(BrokerError::Io(format!("BCryptGenRandom failed: {status}")))
    } else {
        Ok(())
    }
}

pub(crate) fn sandbox_profile_hash() -> [u8; 32] {
    domain_hash(
        b"windows-app-container-profile\0",
        &[
            b"empty-capabilities",
            b"explicit-handle-list",
            b"empty-environment",
            b"appcontainer-lowbox-token",
            b"dep+sehop-mitigations",
            b"job-active-process-1+kill-on-close",
        ],
    )
}

pub(crate) fn spawn_denied_worker(
    source_executable: &Path,
    launch_nonce: &[u8; 16],
) -> Result<SpawnedWorker, BrokerError> {
    let profile_string = format!("Verter.Processor.{}", hex(launch_nonce));
    let profile_name = wide(&profile_string);
    let mut sid = null_mut();
    let hr = unsafe {
        CreateAppContainerProfile(
            profile_name.as_ptr(),
            profile_name.as_ptr(),
            profile_name.as_ptr(),
            null(),
            0,
            &mut sid,
        )
    };
    if hr < 0 {
        return Err(unavailable("CreateAppContainerProfile", Some(hr)));
    }
    let profile = AppContainerProfile {
        name: profile_name,
        sid,
        keep: false,
    };
    let executable = app_container_executable(&profile_string, source_executable, profile.sid)?;
    let pipe_name = wide(format!(r"\\.\pipe\verter-processor-{}", hex(launch_nonce)));
    let server = unsafe {
        CreateNamedPipeW(
            pipe_name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            64 * 1024,
            64 * 1024,
            0,
            null(),
        )
    };
    if server == INVALID_HANDLE_VALUE {
        return Err(io_error("CreateNamedPipeW"));
    }
    let server_guard = OwnedHandle(server);
    let inherit_attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let client = unsafe {
        CreateFileW(
            pipe_name.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            0,
            &inherit_attributes,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if client == INVALID_HANDLE_VALUE {
        return Err(io_error("CreateFileW(worker pipe)"));
    }
    let client_guard = OwnedHandle(client);
    if unsafe { SetHandleInformation(client, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
        return Err(io_error("SetHandleInformation"));
    }

    let job = create_job()?;
    let job_guard = OwnedHandle(job);
    let mut attributes = AttributeList::new(2)?;
    let handles = [client];
    attributes.update(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
        handles.as_ptr().cast(),
        std::mem::size_of_val(&handles),
    )?;
    let mitigations = u64::from(
        PROCESS_CREATION_MITIGATION_POLICY_DEP_ENABLE
            | PROCESS_CREATION_MITIGATION_POLICY_SEHOP_ENABLE,
    );
    attributes.update(
        PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY as usize,
        (&raw const mitigations).cast(),
        std::mem::size_of_val(&mitigations),
    )?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.as_ptr();
    let executable_wide = wide(executable.path().as_os_str());
    let mut command_line = wide(format!(
        "\"{}\" --broker-handle {} --worker-executable \"{}\"",
        executable.path().display(),
        client as usize,
        executable.path().display()
    ));
    let mut process = PROCESS_INFORMATION::default();
    let mut empty_environment = [0_u16; 2];
    let current_directory = wide(
        executable
            .path()
            .parent()
            .ok_or(BrokerError::Protocol("worker executable has no parent"))?,
    );
    let token = create_lowbox_token(profile.sid)?;
    let created = unsafe {
        CreateProcessAsUserW(
            token.0,
            executable_wide.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            empty_environment.as_mut_ptr().cast(),
            current_directory.as_ptr(),
            &startup.StartupInfo,
            &mut process,
        )
    };
    if created == 0 {
        return Err(unavailable(
            "CreateProcessAsUserW(AppContainer)",
            Some(unsafe { GetLastError() } as i32),
        ));
    }
    let process_guard = OwnedHandle(process.hProcess);
    let thread_guard = OwnedHandle(process.hThread);
    if unsafe { AssignProcessToJobObject(job, process.hProcess) } == 0 {
        return Err(io_error("AssignProcessToJobObject"));
    }
    if unsafe { ResumeThread(process.hThread) } == u32::MAX {
        return Err(io_error("ResumeThread"));
    }
    drop(thread_guard);
    drop(client_guard);
    let process_handle = process_guard.into_raw();
    let job_handle = job_guard.into_raw();
    let server_handle = server_guard.into_raw();
    let profile_name = profile.into_name();
    let executable = executable.into_path();
    Ok(SpawnedWorker {
        child: PlatformChild {
            process: process_handle,
            job: job_handle,
            profile_name,
            executable: executable.clone(),
        },
        stream: unsafe { File::from_raw_handle(server_handle as RawHandle) },
        executable,
    })
}

fn create_lowbox_token(
    profile_sid: windows_sys::Win32::Security::PSID,
) -> Result<OwnedHandle, BrokerError> {
    let mut current_token = null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            &mut current_token,
        )
    } == 0
    {
        return Err(unavailable(
            "OpenProcessToken(lowbox)",
            Some(unsafe { GetLastError() } as i32),
        ));
    }
    let current_token = OwnedHandle(current_token);
    let mut lowbox_token = null_mut();
    let status = unsafe {
        NtCreateLowBoxToken(
            &mut lowbox_token,
            current_token.0,
            TOKEN_ALL_ACCESS,
            null(),
            profile_sid,
            0,
            null(),
            0,
            null(),
        )
    };
    if status < 0 {
        return Err(unavailable("NtCreateLowBoxToken", Some(status)));
    }
    Ok(OwnedHandle(lowbox_token))
}

pub(crate) fn wait_readable(
    stream: &mut PlatformStream,
    child: &mut PlatformChild,
    timeout: Duration,
) -> Result<(), BrokerError> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut available = 0_u32;
        if unsafe {
            PeekNamedPipe(
                stream.as_raw_handle() as HANDLE,
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        } != 0
            && available >= 4
        {
            return Ok(());
        }
        if let Some(status) = child.has_exited() {
            return Err(BrokerError::WorkerCrashed(Some(status)));
        }
        if Instant::now() >= deadline {
            return Err(BrokerError::WorkerTimeout);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(crate) fn worker_stream_from_args() -> Result<(PlatformStream, PathBuf), BrokerError> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new("--broker-handle")) {
        return Err(BrokerError::Protocol("missing broker handle"));
    }
    let handle: usize = args
        .next()
        .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
        .ok_or(BrokerError::Protocol("invalid broker handle"))?;
    if args.next().as_deref() != Some(OsStr::new("--worker-executable")) {
        return Err(BrokerError::Protocol("missing worker executable"));
    }
    let executable = args
        .next()
        .map(PathBuf::from)
        .ok_or(BrokerError::Protocol("invalid worker executable"))?;
    let stream = unsafe { File::from_raw_handle(handle as RawHandle) };
    Ok((stream, executable))
}

pub(crate) fn apply_worker_sandbox() -> Result<(), BrokerError> {
    clear_environment()?;
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(unavailable(
            "OpenProcessToken",
            Some(unsafe { GetLastError() } as i32),
        ));
    }
    let token = OwnedHandle(token);
    let mut is_app_container = 0_u32;
    let mut returned = 0_u32;
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenIsAppContainer,
            (&raw mut is_app_container).cast(),
            std::mem::size_of_val(&is_app_container) as u32,
            &mut returned,
        )
    } == 0
        || is_app_container != 1
    {
        return Err(unavailable(
            "TokenIsAppContainer",
            Some(unsafe { GetLastError() } as i32),
        ));
    }
    Ok(())
}

fn clear_environment() -> Result<(), BrokerError> {
    let names: Vec<_> = std::env::vars_os().map(|(name, _)| name).collect();
    for name in names {
        if name.as_encoded_bytes().first() == Some(&b'=') {
            continue;
        }
        let wide_name = wide(&name);
        let cleared = unsafe { SetEnvironmentVariableW(wide_name.as_ptr(), null()) };
        let error = unsafe { GetLastError() };
        if cleared == 0 && error != 203 {
            return Err(unavailable(
                "SetEnvironmentVariableW(empty)",
                Some(error as i32),
            ));
        }
        std::env::remove_var(&name);
    }
    Ok(())
}

pub(crate) fn attempt_child_process() -> bool {
    std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "exit", "0"])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
pub(crate) fn wait_pid_gone_for_test(pid: u32, timeout: Duration) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return true;
        }
        let mut exit_code = 0_u32;
        let exited = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0 && exit_code != 259;
        unsafe { CloseHandle(handle) };
        if exited {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn create_job() -> Result<HANDLE, BrokerError> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(io_error("CreateJobObjectW"));
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    if unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        unsafe { CloseHandle(job) };
        return Err(io_error("SetInformationJobObject"));
    }
    Ok(job)
}

fn app_container_executable(
    profile_name: &str,
    source_executable: &Path,
    profile_sid: windows_sys::Win32::Security::PSID,
) -> Result<StagedExecutable, BrokerError> {
    let public = std::env::var_os("PUBLIC").ok_or(BrokerError::Protocol("PUBLIC unavailable"))?;
    let executable_root = PathBuf::from(public).join("Documents").join(profile_name);
    std::fs::create_dir(&executable_root)?;
    if let Err(error) = grant_profile_read_execute(&executable_root, profile_sid) {
        let _ = std::fs::remove_dir(&executable_root);
        return Err(error);
    }
    let executable = executable_root.join("verter-processor-worker.exe");
    if let Err(error) = std::fs::copy(source_executable, &executable) {
        let _ = std::fs::remove_dir(&executable_root);
        return Err(error.into());
    }
    Ok(StagedExecutable(Some(executable)))
}

fn grant_profile_read_execute(
    path: &Path,
    profile_sid: windows_sys::Win32::Security::PSID,
) -> Result<(), BrokerError> {
    let path = wide(path);
    let mut old_dacl = null_mut();
    let mut security_descriptor = null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old_dacl,
            null_mut(),
            &mut security_descriptor,
        )
    };
    if status != 0 {
        return Err(unavailable("GetNamedSecurityInfoW", Some(status as i32)));
    }
    let security_descriptor = LocalAllocation(security_descriptor);
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: 0,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: profile_sid.cast(),
        },
    };
    let mut new_dacl = null_mut();
    let status = unsafe { SetEntriesInAclW(1, &access, old_dacl, &mut new_dacl) };
    if status != 0 {
        return Err(unavailable("SetEntriesInAclW", Some(status as i32)));
    }
    let new_dacl = LocalAllocation(new_dacl.cast());
    let status = unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_dacl.0.cast(),
            null_mut(),
        )
    };
    drop(new_dacl);
    drop(security_descriptor);
    if status != 0 {
        return Err(unavailable("SetNamedSecurityInfoW", Some(status as i32)));
    }
    Ok(())
}

fn unavailable(operation: &'static str, os_error: Option<i32>) -> BrokerError {
    BrokerError::SandboxUnavailable(SandboxUnavailableEvidence::new(
        ProcessorSandboxKindV1::WindowsAppContainer,
        operation,
        os_error,
    ))
}

fn io_error(operation: &'static str) -> BrokerError {
    BrokerError::Io(format!(
        "{operation} failed: {}",
        io::Error::last_os_error()
    ))
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
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

struct OwnedHandle(HANDLE);

struct LocalAllocation(*mut core::ffi::c_void);

struct StagedExecutable(Option<PathBuf>);

impl StagedExecutable {
    fn path(&self) -> &Path {
        self.0.as_deref().expect("staged executable present")
    }

    fn into_path(mut self) -> PathBuf {
        self.0.take().expect("staged executable present")
    }
}

impl Drop for StagedExecutable {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(&path);
            if let Some(parent) = path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }
}

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

impl OwnedHandle {
    fn into_raw(mut self) -> HANDLE {
        let handle = self.0;
        self.0 = null_mut();
        handle
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

struct AppContainerProfile {
    name: Vec<u16>,
    sid: windows_sys::Win32::Security::PSID,
    keep: bool,
}

impl AppContainerProfile {
    fn into_name(mut self) -> Vec<u16> {
        self.keep = true;
        unsafe { FreeSid(self.sid) };
        self.sid = null_mut();
        std::mem::take(&mut self.name)
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        if !self.sid.is_null() {
            unsafe { FreeSid(self.sid) };
        }
        if !self.keep {
            unsafe { DeleteAppContainerProfile(self.name.as_ptr()) };
        }
    }
}

struct AttributeList {
    storage: Vec<usize>,
}

impl AttributeList {
    fn new(count: u32) -> Result<Self, BrokerError> {
        let mut bytes = 0_usize;
        unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes) };
        let words = bytes.div_ceil(std::mem::size_of::<usize>());
        let mut storage = vec![0_usize; words];
        if unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), count, 0, &mut bytes)
        } == 0
        {
            return Err(io_error("InitializeProcThreadAttributeList"));
        }
        Ok(Self { storage })
    }

    fn as_ptr(&mut self) -> windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }

    fn update(
        &mut self,
        attribute: usize,
        value: *const core::ffi::c_void,
        size: usize,
    ) -> Result<(), BrokerError> {
        if unsafe {
            UpdateProcThreadAttribute(self.as_ptr(), 0, attribute, value, size, null_mut(), null())
        } == 0
        {
            return Err(io_error("UpdateProcThreadAttribute"));
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
    }
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateLowBoxToken(
        token_handle: *mut HANDLE,
        existing_token_handle: HANDLE,
        desired_access: u32,
        object_attributes: *const core::ffi::c_void,
        package_sid: windows_sys::Win32::Security::PSID,
        capability_count: u32,
        capabilities: *const windows_sys::Win32::Security::SID_AND_ATTRIBUTES,
        handle_count: u32,
        handles: *const HANDLE,
    ) -> i32;
}
