//! Windows backend: the child is born inside a job object.
//!
//! `CreateProcessW` receives the job through `PROC_THREAD_ATTRIBUTE_JOB_LIST`
//! and `CREATE_SUSPENDED`, so the child is a job member before its first
//! instruction and every descendant is born inside the job too. The job's
//! limits are a job-wide committed-memory cap (`JOB_OBJECT_LIMIT_JOB_MEMORY`),
//! kill-on-close and no breakaway. The kernel refuses commit past the cap and
//! posts `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT` to the job's completion port, on
//! which the supervisor terminates the job. If the supervisor dies, its job
//! handle closes and the kernel kills the tree.
//!
//! Telemetry is the job's own accounting: `PeakJobMemoryUsed` is the
//! kernel's high-water mark of the tree's commit charge, so the peak does not
//! depend on sampling. At a memory kill that mark includes the request the
//! kernel refused, so it can sit above the cap by up to that request; the
//! commit the tree was granted never exceeds the cap.

use std::ffi::{c_void, OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, SetHandleInformation, FALSE, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    TRUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Console::{
    SetConsoleCtrlHandler, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JobObjectAssociateCompletionPortInformation,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::SystemServices::{
    JOB_OBJECT_MSG_JOB_MEMORY_LIMIT, JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, SetEvent, UpdateProcThreadAttribute,
    WaitForMultipleObjects, CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatus, PostQueuedCompletionStatus, OVERLAPPED,
};

use crate::cli::RunSpec;
use crate::report::{
    Containment, KilledBy, Report, Sample, SampleSeries, EXIT_CANCELLED, EXIT_MEMORY,
    EXIT_SUPERVISOR_ERROR, EXIT_TIMEOUT,
};
use crate::{Fault, Launch, Outputs, FAULT_ENV, MIDRUN_FAULT_OBSERVATION};

const PEAK_METRIC: &str = "job-peak-commit-charge";
const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
/// How long teardown waits for the job to empty before reporting survivors.
const TEARDOWN_LIMIT: Duration = Duration::from_secs(5);
const JOB_KEY: usize = 1;
const STOP_KEY: usize = 2;

/// An owned kernel handle.
struct Owned(HANDLE);

// SAFETY: kernel handles are process-wide and usable from any thread.
unsafe impl Send for Owned {}
// SAFETY: as above; every use is a thread-safe Win32 call.
unsafe impl Sync for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle and closes it once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

/// Why the supervisor killed the tree; the first cause wins.
struct Cause(AtomicU8);

impl Cause {
    fn set(&self, cause: KilledBy) -> bool {
        let code = match cause {
            KilledBy::Memory => 1,
            KilledBy::Timeout => 2,
            KilledBy::Cancel => 3,
            KilledBy::SupervisorError => 4,
            KilledBy::Pressure => 5,
        };
        self.0
            .compare_exchange(0, code, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn get(&self) -> Option<KilledBy> {
        match self.0.load(Ordering::SeqCst) {
            1 => Some(KilledBy::Memory),
            2 => Some(KilledBy::Timeout),
            3 => Some(KilledBy::Cancel),
            4 => Some(KilledBy::SupervisorError),
            5 => Some(KilledBy::Pressure),
            _ => None,
        }
    }
}

/// State shared with the completion-port thread.
struct Shared {
    job: Owned,
    cause: Cause,
    killed_at: Mutex<Option<Instant>>,
}

impl Shared {
    /// Record `cause` (if first) and terminate every process in the job.
    fn kill(&self, cause: KilledBy, exit_code: i32) {
        if self.cause.set(cause) {
            *self.killed_at.lock().unwrap() = Some(Instant::now());
        }
        // SAFETY: the job handle is valid for the lifetime of `Shared`.
        unsafe {
            TerminateJobObject(self.job.0, exit_code as u32);
        }
    }
}

static CANCELLED: AtomicBool = AtomicBool::new(false);
static CANCEL_EVENT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe extern "system" fn on_console_ctrl(ctrl: u32) -> i32 {
    CANCELLED.store(true, Ordering::SeqCst);
    let event = CANCEL_EVENT.load(Ordering::SeqCst);
    if !event.is_null() {
        // SAFETY: the event outlives the handler registration.
        unsafe {
            SetEvent(event);
        }
    }
    if matches!(
        ctrl,
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT
    ) {
        // The process ends when this handler returns; give the supervisor
        // loop time to kill the tree and write the result. Kill-on-close
        // covers the tree even if it does not finish.
        std::thread::sleep(Duration::from_secs(3));
    }
    TRUE
}

pub(crate) fn supervise(launch: Launch<'_>, report: &mut Report) {
    let spec = launch.spec;
    report.peak_metric = Some(PEAK_METRIC);
    report.sample_metric = Some(PEAK_METRIC);
    report.kill_trigger_bytes = Some(spec.mem_limit_bytes);

    let shared = match establish_job(spec.mem_limit_bytes, launch.fault) {
        Ok(shared) => Arc::new(shared),
        Err(error) => {
            report.errors.push(error);
            return;
        }
    };
    let port = match attach_port(&shared.job) {
        Ok(port) => Arc::new(port),
        Err(error) => {
            report.errors.push(error);
            return;
        }
    };
    if let Err(error) = probe_telemetry(&shared.job, spec.mem_limit_bytes, launch.fault) {
        report.errors.push(error);
        return;
    }
    report.containment = Some(Containment::Hard);
    report.overshoot_bound_bytes = Some(0);

    // SAFETY: an unnamed manual-reset event with default security.
    let cancel_event = unsafe { CreateEventW(std::ptr::null(), TRUE, FALSE, std::ptr::null()) };
    if cancel_event.is_null() {
        report.errors.push(format!(
            "cannot create the cancellation event: {}",
            last_error()
        ));
        return;
    }
    let cancel_event = Owned(cancel_event);
    CANCEL_EVENT.store(cancel_event.0, Ordering::SeqCst);
    // SAFETY: registers a handler with the documented signature.
    if unsafe { SetConsoleCtrlHandler(Some(on_console_ctrl), TRUE) } == 0 {
        report.errors.push(format!(
            "cannot install the cancellation handler: {}",
            last_error()
        ));
        return;
    }

    let child = match spawn_in_job(spec, &shared.job, launch.outputs) {
        Ok(child) => child,
        Err(error) => {
            report.errors.push(error);
            finish_handler();
            return;
        }
    };

    let port_thread = {
        let shared = Arc::clone(&shared);
        let port = Arc::clone(&port);
        std::thread::spawn(move || watch_port(&shared, &port))
    };

    if CANCELLED.load(Ordering::SeqCst) {
        shared.kill(KilledBy::Cancel, EXIT_CANCELLED);
        stop_port(&port, port_thread);
        report.killed_by = shared.cause.get();
        finish_handler();
        return;
    }

    let started = Instant::now();
    // SAFETY: the thread handle belongs to the suspended child.
    if unsafe { ResumeThread(child.thread.0) } == u32::MAX {
        let error = last_error();
        shared.kill(KilledBy::SupervisorError, EXIT_SUPERVISOR_ERROR);
        stop_port(&port, port_thread);
        report
            .errors
            .push(format!("cannot release the contained child: {error}"));
        finish_handler();
        return;
    }
    report.launched = true;

    let interval = spec.sample_interval.unwrap_or(DEFAULT_SAMPLE_INTERVAL);
    report.sampling.interval_ms = ms(interval);
    let deadline = started + spec.timeout;
    let mut series = SampleSeries::default();
    let mut next_sample = started;
    let mut last_observation = started;
    let handles = [child.process.0, cancel_event.0];

    loop {
        let now = Instant::now();
        if shared.cause.get().is_none() && now >= deadline {
            shared.kill(KilledBy::Timeout, EXIT_TIMEOUT);
        }
        let killing = shared.cause.get().is_some();
        if killing {
            let killed_at = shared.killed_at.lock().unwrap().unwrap_or(now);
            if now.duration_since(killed_at) > TEARDOWN_LIMIT {
                report
                    .errors
                    .push("the child did not exit after the tree was terminated".to_owned());
                break;
            }
        }
        let until = if killing {
            now + Duration::from_millis(50)
        } else {
            deadline.min(next_sample)
        };
        let wait_ms = until
            .saturating_duration_since(now)
            .as_millis()
            .min(u128::from(INFINITE - 1)) as u32;
        // SAFETY: both handles stay valid for the whole loop.
        let signalled = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), FALSE, wait_ms) };
        if signalled == WAIT_OBJECT_0 {
            break;
        }
        if signalled == WAIT_OBJECT_0 + 1 {
            if !killing {
                shared.kill(KilledBy::Cancel, EXIT_CANCELLED);
            }
            continue;
        }
        if signalled != WAIT_TIMEOUT {
            report
                .errors
                .push(format!("waiting on the child failed: {}", last_error()));
            shared.kill(KilledBy::SupervisorError, EXIT_SUPERVISOR_ERROR);
            continue;
        }
        let now = Instant::now();
        if killing || now < next_sample {
            continue;
        }
        report.sampling.count += 1;
        let observed = if launch.fault == Some(Fault::TelemetryMidrun)
            && report.sampling.count == MIDRUN_FAULT_OBSERVATION
        {
            Err("injected fault".to_owned())
        } else {
            query_extended(&shared.job).map(|info| info.PeakJobMemoryUsed as u64)
        };
        match observed {
            Ok(bytes) => {
                let done = Instant::now();
                series.push(Sample {
                    t_ms: ms(done.duration_since(started)),
                    bytes,
                });
                let sampling = &mut report.sampling;
                sampling.max_sample_age_ms = sampling
                    .max_sample_age_ms
                    .max(ms(done.duration_since(last_observation)));
                sampling.max_sweep_ms = sampling.max_sweep_ms.max(ms(done.duration_since(now)));
                last_observation = done;
            }
            Err(error) => {
                report
                    .errors
                    .push(format!("telemetry lost mid-run: {error}"));
                shared.kill(KilledBy::SupervisorError, EXIT_SUPERVISOR_ERROR);
            }
        }
        next_sample = (next_sample + interval).max(Instant::now());
    }

    let exited = Instant::now();
    report.wall_ms = Some(ms(exited.duration_since(started)));
    if let Some(killed_at) = *shared.killed_at.lock().unwrap() {
        report.termination_latency_ms = Some(ms(exited.saturating_duration_since(killed_at)));
    }
    let mut exit_code = 0u32;
    // SAFETY: the process handle is valid and the out-parameter is a local.
    if unsafe { GetExitCodeProcess(child.process.0, &mut exit_code) } != 0 {
        report.exit_code = Some(i64::from(exit_code));
    } else {
        report.errors.push(format!(
            "cannot read the child's exit status: {}",
            last_error()
        ));
    }

    // Tear down whatever the child left behind, and wait for the job to empty.
    match query_accounting(&shared.job) {
        Ok(accounting) => {
            report.descendants_killed = Some(u64::from(accounting.ActiveProcesses));
            if accounting.ActiveProcesses > 0 {
                // SAFETY: the job handle is valid.
                unsafe {
                    TerminateJobObject(shared.job.0, 1);
                }
            }
        }
        Err(error) => {
            report.errors.push(format!("telemetry: {error}"));
            // SAFETY: the job handle is valid.
            unsafe {
                TerminateJobObject(shared.job.0, 1);
            }
        }
    }
    let teardown_deadline = Instant::now() + TEARDOWN_LIMIT;
    loop {
        match query_accounting(&shared.job) {
            Ok(accounting) if accounting.ActiveProcesses == 0 => break,
            Ok(accounting) if Instant::now() > teardown_deadline => {
                report.errors.push(format!(
                    "{} processes were still alive {TEARDOWN_LIMIT:?} after teardown",
                    accounting.ActiveProcesses
                ));
                break;
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(2)),
            Err(error) => {
                report.errors.push(format!("telemetry: {error}"));
                break;
            }
        }
    }

    stop_port(&port, port_thread);
    report.killed_by = shared.cause.get();

    match query_extended(&shared.job) {
        Ok(info) => {
            let peak = info.PeakJobMemoryUsed as u64;
            report.peak_bytes = Some(peak);
            series.push(Sample {
                t_ms: ms(exited.duration_since(started)),
                bytes: peak,
            });
        }
        Err(error) => report
            .errors
            .push(format!("telemetry: cannot read the job's peak: {error}")),
    }
    match query_accounting(&shared.job) {
        Ok(accounting) => {
            report.process_count = Some(u64::from(accounting.TotalProcesses));
            report.cpu_user_ms = Some(accounting.TotalUserTime as f64 / 10_000.0);
            report.cpu_kernel_ms = Some(accounting.TotalKernelTime as f64 / 10_000.0);
        }
        Err(error) => report.errors.push(format!("telemetry: {error}")),
    }
    report.samples = series.into_vec();
    finish_handler();
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn finish_handler() {
    // SAFETY: removes the handler registered by `supervise`.
    unsafe {
        SetConsoleCtrlHandler(Some(on_console_ctrl), FALSE);
    }
    CANCEL_EVENT.store(std::ptr::null_mut(), Ordering::SeqCst);
}

/// Create the job with its memory cap, kill-on-close and no breakaway.
fn establish_job(mem_limit_bytes: u64, fault: Option<Fault>) -> Result<Shared, String> {
    if fault == Some(Fault::Containment) {
        return Err("cannot establish containment: injected fault".to_owned());
    }
    let limit = usize::try_from(mem_limit_bytes)
        .map_err(|_| format!("cannot establish containment: {mem_limit_bytes} bytes overflows"))?;
    // SAFETY: an unnamed job with default (non-inheritable) security, so no
    // child can hold the job open after the supervisor dies.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(format!(
            "cannot establish containment: CreateJobObjectW failed: {}",
            last_error()
        ));
    }
    let job = Owned(job);
    // SAFETY: an all-zero limit structure is valid (no limits).
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    info.JobMemoryLimit = limit;
    // SAFETY: `info` is the structure the information class expects.
    let set = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if set == 0 {
        return Err(format!(
            "cannot establish containment: SetInformationJobObject failed: {}",
            last_error()
        ));
    }
    Ok(Shared {
        job,
        cause: Cause(AtomicU8::new(0)),
        killed_at: Mutex::new(None),
    })
}

/// Create the completion port that receives the job's limit notifications.
fn attach_port(job: &Owned) -> Result<Owned, String> {
    // SAFETY: creates a fresh port not bound to any file.
    let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1) };
    if port.is_null() {
        return Err(format!(
            "cannot establish containment: CreateIoCompletionPort failed: {}",
            last_error()
        ));
    }
    let port = Owned(port);
    let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
        CompletionKey: JOB_KEY as *mut c_void,
        CompletionPort: port.0,
    };
    // SAFETY: `association` is the structure the information class expects.
    let set = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectAssociateCompletionPortInformation,
            &association as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
        )
    };
    if set == 0 {
        return Err(format!(
            "cannot establish containment: the job's limit notifications are unavailable: {}",
            last_error()
        ));
    }
    Ok(port)
}

/// Read the empty job back: the cap must be in force and the accounting the
/// peak comes from must be readable before anything runs.
fn probe_telemetry(job: &Owned, mem_limit_bytes: u64, fault: Option<Fault>) -> Result<(), String> {
    if fault == Some(Fault::Telemetry) {
        return Err("cannot establish telemetry: injected fault".to_owned());
    }
    let info =
        query_extended(job).map_err(|error| format!("cannot establish telemetry: {error}"))?;
    if info.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_JOB_MEMORY == 0
        || info.JobMemoryLimit as u64 != mem_limit_bytes
    {
        return Err(format!(
            "cannot establish containment: the job reports a memory limit of {} bytes, not {mem_limit_bytes}",
            info.JobMemoryLimit
        ));
    }
    query_accounting(job).map_err(|error| format!("cannot establish telemetry: {error}"))?;
    Ok(())
}

fn query_extended(job: &Owned) -> Result<JOBOBJECT_EXTENDED_LIMIT_INFORMATION, String> {
    // SAFETY: the out-parameter is sized for the information class.
    unsafe {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        if QueryInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(format!(
                "QueryInformationJobObject failed: {}",
                last_error()
            ));
        }
        Ok(info)
    }
}

fn query_accounting(job: &Owned) -> Result<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, String> {
    // SAFETY: the out-parameter is sized for the information class.
    unsafe {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
        if QueryInformationJobObject(
            job.0,
            JobObjectBasicAccountingInformation,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(format!(
                "job accounting is unreadable: QueryInformationJobObject failed: {}",
                last_error()
            ));
        }
        Ok(info)
    }
}

/// A suspended child inside the job.
struct Child {
    process: Owned,
    thread: Owned,
}

/// Create the child suspended and already inside `job`, inheriting only its
/// three standard handles.
fn spawn_in_job(spec: &RunSpec, job: &Owned, outputs: Outputs) -> Result<Child, String> {
    let program = spec.program.to_string_lossy().into_owned();
    let refuse = |why: String| format!("cannot spawn {program}: {why}");
    let lowered = program.to_ascii_lowercase();
    if lowered.ends_with(".bat") || lowered.ends_with(".cmd") {
        return Err(refuse(
            "batch scripts are not launched directly (their argument quoting is unsafe); \
             run `cmd.exe /d /c <script>` explicitly"
                .to_owned(),
        ));
    }
    let stdin = OpenOptions::new()
        .read(true)
        .open("NUL")
        .map_err(|error| refuse(format!("cannot open NUL for stdin: {error}")))?;
    let Outputs { stdout, stderr } = outputs;
    let inherited: [HANDLE; 3] = [raw(&stdin), raw(&stdout), raw(&stderr)];
    for handle in inherited {
        // SAFETY: each handle is owned by a live `File` above.
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(refuse(format!(
                "cannot make a standard handle inheritable: {}",
                last_error()
            )));
        }
    }

    let application = resolve_path_program(&spec.program)
        .map_err(|error| refuse(format!("cannot resolve the program path: {error}")))?;
    let program_for_line = application
        .as_ref()
        .map_or(spec.program.as_os_str(), |path| path.as_os_str());
    let mut command_line = command_line(program_for_line, &spec.args);
    let application_wide: Option<Vec<u16>> =
        application.as_ref().map(|path| wide(path.as_os_str()));
    let environment = environment_block(&spec.env);
    let cwd: Option<Vec<u16>> = spec.cwd.as_ref().map(|cwd| wide(cwd.as_os_str()));

    // SAFETY: the attribute list is sized by the first call, initialised
    // once, filled with pointers that outlive `CreateProcessW`, and deleted
    // before its buffer is freed.
    unsafe {
        let mut size = 0usize;
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut size);
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let list = buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(list, 2, 0, &mut size) == 0 {
            return Err(refuse(format!(
                "InitializeProcThreadAttributeList failed: {}",
                last_error()
            )));
        }
        let jobs: [HANDLE; 1] = [job.0];
        let attached = UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            jobs.as_ptr() as *const c_void,
            std::mem::size_of_val(&jobs),
            std::ptr::null_mut(),
            std::ptr::null(),
        ) != 0
            && UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_ptr() as *const c_void,
                std::mem::size_of_val(&inherited),
                std::ptr::null_mut(),
                std::ptr::null(),
            ) != 0;
        if !attached {
            let error = last_error();
            DeleteProcThreadAttributeList(list);
            return Err(format!(
                "cannot establish containment: the job cannot be attached at process creation: {error}"
            ));
        }

        let mut startup: STARTUPINFOEXW = std::mem::zeroed();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = inherited[0];
        startup.StartupInfo.hStdOutput = inherited[1];
        startup.StartupInfo.hStdError = inherited[2];
        startup.lpAttributeList = list;
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        let created = CreateProcessW(
            application_wide
                .as_ref()
                .map_or(std::ptr::null(), |path| path.as_ptr()),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            TRUE,
            CREATE_SUSPENDED
                | CREATE_NEW_PROCESS_GROUP
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr() as *const c_void,
            cwd.as_ref().map_or(std::ptr::null(), |cwd| cwd.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        );
        let error = last_error();
        DeleteProcThreadAttributeList(list);
        if created == 0 {
            // ERROR_COMMITMENT_LIMIT at creation: inside the job, the new
            // process's own startup commit already exceeds the cap.
            const ERROR_COMMITMENT_LIMIT: i32 = 1455;
            let hint = if error.raw_os_error() == Some(ERROR_COMMITMENT_LIMIT) {
                " (the memory cap may be below the program's startup commit)"
            } else {
                ""
            };
            return Err(refuse(format!("{error}{hint}")));
        }
        Ok(Child {
            process: Owned(info.hProcess),
            thread: Owned(info.hThread),
        })
    }
}

/// A program named by a path (it contains a separator) is resolved against the
/// supervisor's working directory, with `.exe` appended when the bare path
/// does not exist, and passed to `CreateProcessW` as the application name.
/// A bare name is left to `CreateProcessW`'s own search.
fn resolve_path_program(program: &OsStr) -> std::io::Result<Option<std::path::PathBuf>> {
    let text = program.to_string_lossy();
    if !text.contains(['/', '\\']) {
        return Ok(None);
    }
    let absolute = std::path::absolute(program)?;
    if !absolute.exists() && absolute.extension().is_none() {
        let with_exe = absolute.with_extension("exe");
        if with_exe.exists() {
            return Ok(Some(with_exe));
        }
    }
    Ok(Some(absolute))
}

fn raw(file: &File) -> HANDLE {
    file.as_raw_handle() as HANDLE
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// The child's command line, quoted so the Microsoft C runtime's
/// `CommandLineToArgvW` rules give back exactly `program` and `args`.
fn command_line(program: &OsStr, args: &[OsString]) -> Vec<u16> {
    let mut line: Vec<u16> = Vec::new();
    for (index, arg) in std::iter::once(program)
        .chain(args.iter().map(OsString::as_os_str))
        .enumerate()
    {
        if index > 0 {
            line.push(u16::from(b' '));
        }
        quote_into(&mut line, arg, index == 0);
    }
    line.push(0);
    line
}

fn quote_into(line: &mut Vec<u16>, arg: &OsStr, is_program: bool) {
    let units: Vec<u16> = arg.encode_wide().collect();
    let needs_quotes = units.is_empty()
        || units.iter().any(|&unit| {
            unit == u16::from(b' ') || unit == u16::from(b'\t') || unit == u16::from(b'"')
        });
    if !needs_quotes {
        line.extend_from_slice(&units);
        return;
    }
    line.push(u16::from(b'"'));
    if is_program {
        // The program name is parsed without escapes; a quote cannot occur in
        // a Windows path.
        line.extend_from_slice(&units);
    } else {
        let mut backslashes = 0usize;
        for &unit in &units {
            if unit == u16::from(b'\\') {
                backslashes += 1;
                continue;
            }
            if unit == u16::from(b'"') {
                line.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2 + 1));
            } else {
                line.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
            }
            backslashes = 0;
            line.push(unit);
        }
        line.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
    }
    line.push(u16::from(b'"'));
}

/// The inherited environment with the `--env` overrides applied (names
/// compare case-insensitively, as Windows does) and the fault hook removed,
/// as a sorted, double-NUL-terminated UTF-16 block.
fn environment_block(overrides: &[(OsString, OsString)]) -> Vec<u16> {
    let key = |name: &OsStr| name.to_string_lossy().to_uppercase();
    let mut vars: std::collections::BTreeMap<String, (OsString, OsString)> =
        std::collections::BTreeMap::new();
    for (name, value) in std::env::vars_os() {
        vars.insert(key(&name), (name, value));
    }
    for (name, value) in overrides {
        vars.insert(key(name), (name.clone(), value.clone()));
    }
    vars.remove(&FAULT_ENV.to_uppercase());
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in vars.values() {
        block.extend(name.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// Handle the job's notifications until told to stop.
fn watch_port(shared: &Shared, port: &Owned) {
    loop {
        let mut message = 0u32;
        let mut key = 0usize;
        let mut overlapped: *mut OVERLAPPED = std::ptr::null_mut();
        // SAFETY: all out-parameters are valid locals.
        let ok = unsafe {
            GetQueuedCompletionStatus(port.0, &mut message, &mut key, &mut overlapped, INFINITE)
        };
        if ok == 0 || key == STOP_KEY {
            return;
        }
        if key == JOB_KEY
            && matches!(
                message,
                JOB_OBJECT_MSG_JOB_MEMORY_LIMIT | JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT
            )
        {
            shared.kill(KilledBy::Memory, EXIT_MEMORY);
        }
    }
}

/// Stop the port thread after it has drained every queued notification.
fn stop_port(port: &Owned, thread: std::thread::JoinHandle<()>) {
    // SAFETY: posts a packet to a valid port.
    unsafe {
        PostQueuedCompletionStatus(port.0, 0, STOP_KEY, std::ptr::null());
    }
    thread.join().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(program: &str, args: &[&str]) -> String {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let mut units = command_line(OsStr::new(program), &args);
        units.pop();
        String::from_utf16(&units).unwrap()
    }

    #[test]
    fn arguments_are_quoted_for_the_c_runtime() {
        assert_eq!(line("C:\\a b\\p.exe", &["x"]), "\"C:\\a b\\p.exe\" x");
        assert_eq!(line("p", &["", "a b", "q\"x"]), "p \"\" \"a b\" \"q\\\"x\"");
        assert_eq!(line("p", &["tail\\ dir\\"]), "p \"tail\\ dir\\\\\"");
        assert_eq!(line("p", &["plain\\path"]), "p plain\\path");
        assert_ne!(line("p", &["a b"]), "p a b");
    }
}
