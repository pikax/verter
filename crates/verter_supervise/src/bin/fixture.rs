//! The process tree the supervisor's integration tests run under
//! `verter-supervise`. Each mode is one behaviour a real probe can show.
//!
//! ```text
//! exit <code>                        exit with <code>
//! stdio <out> <err>                  write <out> to stdout and <err> to stderr
//! env <KEY>                          print the working directory and $KEY
//! touch <path>                       create <path> (proves the child ran)
//! sleep <ms>                         sleep, then exit 0
//! alloc <chunk-mb>                   commit and touch <chunk-mb> chunks until
//!                                    an allocation is refused, then wait to be
//!                                    killed (never exits on its own)
//! spawn-wait <pidfile> <mode...>     publish its own pid to <pidfile>.parent,
//!                                    start a grandchild running <mode...>,
//!                                    publish its pid, and wait forever
//! spawn-exit <pidfile> <mode...>     start a grandchild, publish its pid, and
//!                                    exit 0 at once, orphaning it
//! spawn-breakaway <pidfile> <mode...> (Windows) try to start the grandchild
//!                                    outside the job, then wait forever
//! spawn-setsid <pidfile> <mode...>   (Unix) start the grandchild in a new
//!                                    session, leaving the process group, then
//!                                    wait forever
//! ```

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, rest) = args
        .split_first()
        .unwrap_or_else(|| panic!("fixture needs a mode"));
    match mode.as_str() {
        "exit" => std::process::exit(rest[0].parse().expect("exit code")),
        "stdio" => {
            print!("{}", rest[0]);
            eprint!("{}", rest[1]);
            std::io::stdout().flush().unwrap();
            std::io::stderr().flush().unwrap();
        }
        "env" => {
            let cwd = std::env::current_dir().expect("cwd");
            let value = std::env::var(&rest[0]).unwrap_or_else(|_| "<unset>".to_owned());
            println!("cwd={}", cwd.display());
            println!("{}={value}", rest[0]);
        }
        "touch" => {
            verter_supervise::disk::write(&rest[0], b"ran").expect("touch");
        }
        "sleep" => std::thread::sleep(Duration::from_millis(rest[0].parse().expect("ms"))),
        "alloc" => alloc(rest[0].parse().expect("chunk MiB")),
        "spawn-wait" => {
            let mut child = spawn_grandchild(&rest[0], &rest[1..], Launch::Plain);
            child.wait().ok();
            wait_forever();
        }
        "spawn-exit" => {
            let child = spawn_grandchild(&rest[0], &rest[1..], Launch::Plain);
            std::mem::forget(child);
        }
        "spawn-breakaway" => {
            let mut child = spawn_grandchild(&rest[0], &rest[1..], Launch::Breakaway);
            child.wait().ok();
            wait_forever();
        }
        "spawn-setsid" => {
            let mut child = spawn_grandchild(&rest[0], &rest[1..], Launch::NewSession);
            child.wait().ok();
            wait_forever();
        }
        other => panic!("unknown fixture mode {other:?}"),
    }
}

fn alloc(chunk_mib: usize) -> ! {
    let chunk = chunk_mib * 1024 * 1024;
    let mut held: Vec<Vec<u8>> = Vec::new();
    let mut total = 0usize;
    loop {
        let mut block: Vec<u8> = Vec::new();
        if block.try_reserve_exact(chunk).is_err() {
            println!("allocation refused after {total} bytes");
            std::io::stdout().flush().ok();
            wait_forever();
        }
        // Write every byte so the memory is resident, not only reserved.
        block.resize(chunk, 1);
        total += chunk;
        held.push(block);
        println!("held {total}");
        std::io::stdout().flush().ok();
    }
}

fn wait_forever() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Launch {
    Plain,
    Breakaway,
    NewSession,
}

fn spawn_grandchild(pidfile: &str, mode: &[String], launch: Launch) -> std::process::Child {
    let exe = std::env::current_exe().expect("fixture path");
    let mut command = Command::new(exe);
    command
        .args(mode)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    publish_pid(
        &Path::new(pidfile).with_extension("parent"),
        std::process::id(),
    );
    let child = match launch {
        Launch::Plain => command.spawn().expect("spawn grandchild"),
        Launch::Breakaway => spawn_breakaway(&mut command),
        Launch::NewSession => spawn_new_session(&mut command),
    };
    publish_pid(Path::new(pidfile), child.id());
    child
}

#[cfg(windows)]
fn spawn_breakaway(command: &mut Command) -> std::process::Child {
    use std::os::windows::process::CommandExt;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    command.creation_flags(CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Ok(child) => {
            println!("breakaway permitted");
            child
        }
        Err(error) => {
            println!("breakaway refused: {error}");
            command.creation_flags(0);
            command.spawn().expect("spawn grandchild")
        }
    }
}

#[cfg(not(windows))]
fn spawn_breakaway(_command: &mut Command) -> std::process::Child {
    panic!("spawn-breakaway is a Windows mode")
}

#[cfg(unix)]
fn spawn_new_session(command: &mut Command) -> std::process::Child {
    use std::os::unix::process::CommandExt;
    // SAFETY: `setsid` is async-signal-safe and touches no parent state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().expect("spawn grandchild")
}

#[cfg(not(unix))]
fn spawn_new_session(_command: &mut Command) -> std::process::Child {
    panic!("spawn-setsid is a Unix mode")
}

/// Write the pid so a reader never sees a partial file.
fn publish_pid(pidfile: &Path, pid: u32) {
    let staging = pidfile.with_extension("staging");
    verter_supervise::disk::write(&staging, pid.to_string()).expect("write pid");
    verter_supervise::disk::rename(&staging, pidfile).expect("publish pid");
}
