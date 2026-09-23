fn main() {
    // Embed build timestamp so the binary can log when it was compiled.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Format as ISO 8601 UTC (YYYY-MM-DD HH:MM:SS UTC)
    let secs_per_day = 86400u64;
    let secs_per_hour = 3600u64;
    let secs_per_min = 60u64;

    let days = now / secs_per_day;
    let time_of_day = now % secs_per_day;
    let hour = time_of_day / secs_per_hour;
    let min = (time_of_day % secs_per_hour) / secs_per_min;
    let sec = time_of_day % secs_per_min;

    // Days since epoch to date (simplified algorithm)
    let (year, month, day) = days_to_date(days);

    let date_str = format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}:{sec:02} UTC");
    println!("cargo:rustc-env=VERTER_BUILD_DATE={date_str}");

    embed_windows_manifest();
}

/// Embed `verter-lsp.manifest` into the server executable on MSVC targets,
/// opting the process into the Windows segment heap (see the manifest for
/// why). The target, not the host, decides: a cross build for a non-MSVC
/// target skips it. No `rerun-if-changed` is emitted on purpose: that would
/// stop this script re-running on other package changes and freeze
/// `VERTER_BUILD_DATE`; the manifest lives in the package, so editing it
/// already re-runs the script.
fn embed_windows_manifest() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let manifest = std::path::Path::new(&manifest_dir).join("verter-lsp.manifest");
    println!("cargo:rustc-link-arg-bin=verter-lsp=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=verter-lsp=/MANIFESTINPUT:{}",
        manifest.display()
    );
}

fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
