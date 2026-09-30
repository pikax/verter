//! The result document (`result.json`, schema 1) and the supervisor's exit
//! code.
//!
//! Telemetry that could not be read is `null`, never `0`: a missing peak must
//! not read as "used no memory".

use std::path::Path;

use serde::Serialize;

use crate::cli::RunSpec;

/// The supervisor's exit code when it could not launch, contain or observe
/// the child (the run is invalid; `errors` says why).
pub const EXIT_SUPERVISOR_ERROR: i32 = 125;
/// The child ran past `--timeout-ms` and the tree was killed.
pub const EXIT_TIMEOUT: i32 = 124;
/// The tree reached the memory cap and was killed.
pub const EXIT_MEMORY: i32 = 137;
/// The supervisor was cancelled (Ctrl-C, Ctrl-Break, SIGINT, SIGTERM, SIGHUP)
/// and killed the tree.
pub const EXIT_CANCELLED: i32 = 130;

/// Why the supervisor killed the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KilledBy {
    #[serde(rename = "memory")]
    Memory,
    #[serde(rename = "timeout")]
    Timeout,
    #[serde(rename = "cancel")]
    Cancel,
    /// Containment or telemetry was lost mid-run (fail closed): the
    /// measurement is invalid.
    #[serde(rename = "supervisor-error")]
    SupervisorError,
    /// The host reported memory pressure while a sampled backend was running.
    #[serde(rename = "pressure")]
    Pressure,
}

impl KilledBy {
    pub fn exit_code(self) -> i32 {
        match self {
            KilledBy::Memory => EXIT_MEMORY,
            KilledBy::Timeout => EXIT_TIMEOUT,
            KilledBy::Cancel => EXIT_CANCELLED,
            KilledBy::SupervisorError | KilledBy::Pressure => EXIT_SUPERVISOR_ERROR,
        }
    }
}

/// How strongly the tree's memory is contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Containment {
    /// The kernel enforces the cap on the whole tree; allocation past it is
    /// refused and the tree is killed.
    #[serde(rename = "hard")]
    Hard,
    /// The supervisor samples the tree and kills it on a breach. There is no
    /// guaranteed overshoot bound.
    #[serde(rename = "sampled")]
    Sampled,
}

/// One telemetry observation, relative to the child's release.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Sample {
    #[serde(rename = "tMs")]
    pub t_ms: f64,
    pub bytes: u64,
}

/// A bounded telemetry series: when full it keeps every other sample and
/// halves its recording rate, so a long run keeps an even outline.
#[derive(Debug, Default)]
pub struct SampleSeries {
    samples: Vec<Sample>,
    stride: u64,
    seen: u64,
}

const MAX_SAMPLES: usize = 4096;

impl SampleSeries {
    pub fn push(&mut self, sample: Sample) {
        if self.stride == 0 {
            self.stride = 1;
        }
        let keep = self.seen.is_multiple_of(self.stride);
        self.seen += 1;
        if !keep {
            return;
        }
        if self.samples.len() == MAX_SAMPLES {
            let mut index = 0;
            self.samples.retain(|_| {
                index += 1;
                index % 2 == 1
            });
            self.stride *= 2;
        }
        self.samples.push(sample);
    }

    pub fn into_vec(self) -> Vec<Sample> {
        self.samples
    }
}

/// Sampling statistics: how many observations were taken and how stale the
/// supervisor's view of the tree ever became.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct SamplingStats {
    #[serde(rename = "intervalMs")]
    pub interval_ms: f64,
    pub count: u64,
    /// The longest time between two completed observations.
    #[serde(rename = "maxSampleAgeMs")]
    pub max_sample_age_ms: f64,
    /// The longest single observation (enumeration plus reads).
    #[serde(rename = "maxSweepMs")]
    pub max_sweep_ms: f64,
}

/// The result document.
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    /// Whether the child was released to run. `false` means the supervisor
    /// refused or failed before the program executed any instruction.
    pub launched: bool,
    #[serde(rename = "wallMs")]
    pub wall_ms: Option<f64>,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i64>,
    pub signal: Option<i32>,
    #[serde(rename = "killedBy")]
    pub killed_by: Option<KilledBy>,
    #[serde(rename = "memLimitBytes")]
    pub mem_limit_bytes: u64,
    /// The footprint at which a sampled backend kills (the cap minus its
    /// headroom). Equal to the cap on a hard backend.
    #[serde(rename = "killTriggerBytes")]
    pub kill_trigger_bytes: Option<u64>,
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: u64,
    #[serde(rename = "peakBytes")]
    pub peak_bytes: Option<u64>,
    #[serde(rename = "peakMetric")]
    pub peak_metric: Option<&'static str>,
    /// What each entry of `samples` measures.
    #[serde(rename = "sampleMetric")]
    pub sample_metric: Option<&'static str>,
    pub containment: Option<Containment>,
    pub backend: &'static str,
    /// The largest guaranteed overshoot past the cap: `0` on a hard backend,
    /// `null` on a sampled one (sampling proves no bound).
    #[serde(rename = "overshootBoundBytes")]
    pub overshoot_bound_bytes: Option<u64>,
    /// How far the observed peak went past the cap, when it did.
    #[serde(rename = "observedOvershootBytes")]
    pub observed_overshoot_bytes: Option<u64>,
    /// Time from the supervisor deciding to kill to the tree being gone.
    #[serde(rename = "terminationLatencyMs")]
    pub termination_latency_ms: Option<f64>,
    pub sampling: SamplingStats,
    pub samples: Vec<Sample>,
    /// Processes that ever ran in the tree, the child included.
    #[serde(rename = "processCount")]
    pub process_count: Option<u64>,
    /// Descendants still alive when the child exited, killed by teardown.
    #[serde(rename = "descendantsKilled")]
    pub descendants_killed: Option<u64>,
    #[serde(rename = "cpuUserMs")]
    pub cpu_user_ms: Option<f64>,
    #[serde(rename = "cpuKernelMs")]
    pub cpu_kernel_ms: Option<f64>,
    #[serde(rename = "stdoutPath")]
    pub stdout_path: String,
    #[serde(rename = "stderrPath")]
    pub stderr_path: String,
    pub errors: Vec<String>,
}

impl Report {
    pub fn new(spec: &RunSpec, backend: &'static str) -> Self {
        Report {
            schema: 1,
            program: spec.program.to_string_lossy().into_owned(),
            args: spec
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            cwd: spec
                .cwd
                .as_ref()
                .map(|cwd| cwd.to_string_lossy().into_owned()),
            started_at: rfc3339_now(),
            launched: false,
            wall_ms: None,
            exit_code: None,
            signal: None,
            killed_by: None,
            mem_limit_bytes: spec.mem_limit_bytes,
            kill_trigger_bytes: None,
            timeout_ms: spec.timeout.as_millis() as u64,
            peak_bytes: None,
            peak_metric: None,
            sample_metric: None,
            containment: None,
            backend,
            overshoot_bound_bytes: None,
            observed_overshoot_bytes: None,
            termination_latency_ms: None,
            sampling: SamplingStats::default(),
            samples: Vec::new(),
            process_count: None,
            descendants_killed: None,
            cpu_user_ms: None,
            cpu_kernel_ms: None,
            stdout_path: spec.stdout_path().to_string_lossy().into_owned(),
            stderr_path: spec.stderr_path().to_string_lossy().into_owned(),
            errors: Vec::new(),
        }
    }

    /// The supervisor's exit code for this report.
    pub fn exit_code(&self) -> i32 {
        if let Some(killed_by) = self.killed_by {
            return killed_by.exit_code();
        }
        if !self.launched || !self.errors.is_empty() {
            return EXIT_SUPERVISOR_ERROR;
        }
        if let Some(signal) = self.signal {
            return 128 + signal;
        }
        match self.exit_code {
            Some(code) => code as i32,
            None => EXIT_SUPERVISOR_ERROR,
        }
    }

    /// Record how far a sampled backend let the tree go past the cap. A hard
    /// backend grants no commit past the cap (its peak charge can still count
    /// a refused request), so it records none.
    pub fn settle_overshoot(&mut self) {
        if self.containment != Some(Containment::Sampled) {
            return;
        }
        self.observed_overshoot_bytes = self
            .peak_bytes
            .map(|peak| peak.saturating_sub(self.mem_limit_bytes))
            .filter(|overshoot| *overshoot > 0);
    }

    /// Write the document atomically: a reader sees either nothing or the
    /// complete result.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            crate::disk::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let staging = path.with_extension(format!("tmp-{}", std::process::id()));
        crate::disk::write(&staging, text)?;
        crate::disk::rename(&staging, path)
    }
}

/// The current UTC time as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_match_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_ne!(civil_from_days(19_783), (2024, 2, 30));
    }

    #[test]
    fn a_full_series_halves_its_rate_and_stays_bounded() {
        let mut series = SampleSeries::default();
        for index in 0..(MAX_SAMPLES as u64 * 3) {
            series.push(Sample {
                t_ms: index as f64,
                bytes: index,
            });
        }
        let samples = series.into_vec();
        assert!(samples.len() <= MAX_SAMPLES);
        assert!(samples.len() > MAX_SAMPLES / 2);
        assert_eq!(samples[0].bytes, 0);
        assert!(samples.windows(2).all(|pair| pair[0].bytes < pair[1].bytes));
    }
}
