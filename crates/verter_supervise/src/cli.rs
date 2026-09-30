//! Command-line contract.
//!
//! ```text
//! verter-supervise run --mem-mb <N> --timeout-ms <T> --out <result.json>
//!     [--sample-ms <S>] [--cwd <dir>] [--env K=V ...]
//!     [--allow-sampled] [--host-reserve-mb <R>] -- <program> <args...>
//! ```
//!
//! Every limit must be a positive integer: a zero, missing or unparsable
//! limit is a usage error, never "unlimited".
//!
//! `--allow-sampled` is the caller's explicit consent to a backend whose cap is
//! enforced by sampling rather than by the kernel (macOS). Without it such a
//! backend refuses to launch. `--host-reserve-mb` is the memory a sampled
//! backend keeps free for the host: it refuses a cap above physical memory
//! minus the reserve.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

/// A validated `run` request.
#[derive(Debug, Clone)]
pub struct RunSpec {
    /// The memory cap for the whole process tree, in bytes.
    pub mem_limit_bytes: u64,
    /// The wall-clock deadline measured from the moment the child is released.
    pub timeout: Duration,
    /// Where the result document is written. The child's stdout and stderr
    /// land next to it.
    pub out: PathBuf,
    /// The telemetry sampling interval. `None` picks the backend's default.
    pub sample_interval: Option<Duration>,
    /// The child's working directory; the supervisor's own when absent.
    pub cwd: Option<PathBuf>,
    /// Extra environment for the child, applied over the inherited one.
    pub env: Vec<(OsString, OsString)>,
    /// Consent to a sampled (not kernel-enforced) memory cap.
    pub allow_sampled: bool,
    /// Host memory a sampled backend keeps free, in bytes. `None` picks the
    /// backend's default.
    pub host_reserve_bytes: Option<u64>,
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl RunSpec {
    /// `<dir>/<stem>.stdout.log` for `<dir>/<stem>.json`.
    pub fn stdout_path(&self) -> PathBuf {
        self.sibling("stdout.log")
    }

    /// `<dir>/<stem>.stderr.log` for `<dir>/<stem>.json`.
    pub fn stderr_path(&self) -> PathBuf {
        self.sibling("stderr.log")
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let stem = self
            .out
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "result".to_owned());
        self.out.with_file_name(format!("{stem}.{suffix}"))
    }
}

/// A usage error, with the result path when it was parsed before the error.
#[derive(Debug)]
pub struct UsageError {
    pub message: String,
    pub out: Option<PathBuf>,
}

pub const USAGE: &str =
    "usage: verter-supervise run --mem-mb <N> --timeout-ms <T> --out <result.json> \
[--sample-ms <S>] [--cwd <dir>] [--env K=V ...] [--allow-sampled] [--host-reserve-mb <R>] \
-- <program> <args...>";

/// Parse the arguments after the executable name.
pub fn parse(args: Vec<OsString>) -> Result<RunSpec, UsageError> {
    let mut iter = args.into_iter();
    let mut out: Option<PathBuf> = None;
    let fail = |message: String, out: &Option<PathBuf>| UsageError {
        message,
        out: out.clone(),
    };

    match iter.next() {
        Some(sub) if sub == "run" => {}
        Some(other) => {
            return Err(fail(
                format!("unknown subcommand {:?}; {USAGE}", other.to_string_lossy()),
                &out,
            ))
        }
        None => return Err(fail(USAGE.to_owned(), &out)),
    }

    let mut mem_mb: Option<u64> = None;
    let mut timeout_ms: Option<u64> = None;
    let mut sample_ms: Option<u64> = None;
    let mut host_reserve_mb: Option<u64> = None;
    let mut cwd: Option<PathBuf> = None;
    let mut env = Vec::new();
    let mut allow_sampled = false;
    let mut command: Option<Vec<OsString>> = None;

    while let Some(arg) = iter.next() {
        if arg == "--" {
            command = Some(iter.by_ref().collect());
            break;
        }
        let flag = arg.to_string_lossy().into_owned();
        if flag == "--allow-sampled" {
            allow_sampled = true;
            continue;
        }
        let mut value = || {
            iter.next()
                .ok_or_else(|| fail(format!("{flag} needs a value; {USAGE}"), &out))
        };
        match flag.as_str() {
            "--mem-mb" => mem_mb = Some(positive(&flag, value()?, &out)?),
            "--timeout-ms" => timeout_ms = Some(positive(&flag, value()?, &out)?),
            "--sample-ms" => sample_ms = Some(positive(&flag, value()?, &out)?),
            "--host-reserve-mb" => host_reserve_mb = Some(positive(&flag, value()?, &out)?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--cwd" => cwd = Some(PathBuf::from(value()?)),
            "--env" => {
                let entry = value()?;
                let text = entry.to_string_lossy().into_owned();
                match text.split_once('=') {
                    Some((key, val)) if !key.is_empty() => {
                        env.push((OsString::from(key), OsString::from(val)))
                    }
                    _ => return Err(fail(format!("--env expects KEY=VALUE, got {text:?}"), &out)),
                }
            }
            _ => return Err(fail(format!("unknown option {flag:?}; {USAGE}"), &out)),
        }
    }

    let out_path = out
        .clone()
        .ok_or_else(|| fail(format!("--out is required; {USAGE}"), &out))?;
    let mem_mb = mem_mb.ok_or_else(|| fail(format!("--mem-mb is required; {USAGE}"), &out))?;
    let timeout_ms =
        timeout_ms.ok_or_else(|| fail(format!("--timeout-ms is required; {USAGE}"), &out))?;
    let mut command = command
        .filter(|command| !command.is_empty())
        .ok_or_else(|| fail(format!("a program is required after `--`; {USAGE}"), &out))?;
    let mem_limit_bytes = megabytes(mem_mb, "--mem-mb", &out)?;
    let host_reserve_bytes = match host_reserve_mb {
        Some(mb) => Some(megabytes(mb, "--host-reserve-mb", &out)?),
        None => None,
    };
    let program = command.remove(0);

    Ok(RunSpec {
        mem_limit_bytes,
        timeout: Duration::from_millis(timeout_ms),
        out: out_path,
        sample_interval: sample_ms.map(Duration::from_millis),
        cwd,
        env,
        allow_sampled,
        host_reserve_bytes,
        program,
        args: command,
    })
}

fn positive(flag: &str, value: OsString, out: &Option<PathBuf>) -> Result<u64, UsageError> {
    let text = value.to_string_lossy();
    match text.parse::<u64>() {
        Ok(number) if number > 0 => Ok(number),
        _ => Err(UsageError {
            message: format!("{flag} must be a positive integer, got {text:?}"),
            out: out.clone(),
        }),
    }
}

fn megabytes(mb: u64, flag: &str, out: &Option<PathBuf>) -> Result<u64, UsageError> {
    mb.checked_mul(1024 * 1024).ok_or_else(|| UsageError {
        message: format!("{flag} {mb} overflows a byte count"),
        out: out.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_documented_invocation_parses() {
        let spec = parse(args(&[
            "run",
            "--mem-mb",
            "8192",
            "--timeout-ms",
            "120000",
            "--out",
            "r/result.json",
            "--sample-ms",
            "5",
            "--cwd",
            "w",
            "--env",
            "A=b=c",
            "--",
            "tsc",
            "-p",
            ".",
        ]))
        .expect("valid invocation");
        assert_eq!(spec.mem_limit_bytes, 8192 * 1024 * 1024);
        assert_eq!(spec.timeout, Duration::from_millis(120_000));
        assert_eq!(spec.sample_interval, Some(Duration::from_millis(5)));
        assert_eq!(spec.env, vec![(OsString::from("A"), OsString::from("b=c"))]);
        assert_eq!(spec.program, OsString::from("tsc"));
        assert_eq!(spec.args, args(&["-p", "."]));
        assert!(!spec.allow_sampled);
        assert_eq!(spec.stdout_path(), PathBuf::from("r/result.stdout.log"));
        assert_eq!(spec.stderr_path(), PathBuf::from("r/result.stderr.log"));
    }

    #[test]
    fn zero_missing_and_negative_limits_are_usage_errors() {
        for bad in [
            &[
                "run",
                "--mem-mb",
                "0",
                "--timeout-ms",
                "1",
                "--out",
                "o.json",
                "--",
                "p",
            ][..],
            &[
                "run",
                "--mem-mb",
                "-5",
                "--timeout-ms",
                "1",
                "--out",
                "o.json",
                "--",
                "p",
            ][..],
            &["run", "--timeout-ms", "1", "--out", "o.json", "--", "p"][..],
            &[
                "run",
                "--mem-mb",
                "1",
                "--timeout-ms",
                "inf",
                "--out",
                "o.json",
                "--",
                "p",
            ][..],
            &[
                "run",
                "--mem-mb",
                "1",
                "--timeout-ms",
                "1",
                "--out",
                "o.json",
                "--",
            ][..],
            &["run", "--mem-mb", "1", "--timeout-ms", "1", "--", "p"][..],
        ] {
            assert!(parse(args(bad)).is_err(), "{bad:?} must be refused");
        }
        let error = parse(args(&[
            "run",
            "--out",
            "o.json",
            "--mem-mb",
            "0",
            "--timeout-ms",
            "1",
            "--",
            "p",
        ]))
        .unwrap_err();
        assert_eq!(error.out, Some(PathBuf::from("o.json")));
    }
}
