//! Architecture guard: `.config/nextest.toml` must configure a slow-timeout
//! period matching the advertised hang-protection budget (60s x 3 = 180s) for
//! BOTH the `default` and `ci` profiles. A period below the advertised value
//! terminates valid slow-but-legitimate tests on a memory-constrained host,
//! corrupting the workspace gate with spurious timeouts. This hermetic guard
//! parses the committed config and fails if the effective period regresses.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run `git rev-parse --show-toplevel`");
    assert!(
        out.status.success(),
        "git rev-parse --show-toplevel failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    PathBuf::from(String::from_utf8(out.stdout).expect("utf8 toplevel").trim())
}

#[test]
fn nextest_slow_timeout_period_is_60s_for_both_profiles() {
    let path = repo_root().join(".config").join("nextest.toml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let cfg: toml::Value = toml::from_str(&text).expect("parse nextest.toml");
    let profile = cfg.get("profile").expect("[profile.*] table present");
    for name in ["default", "ci"] {
        let st = profile
            .get(name)
            .and_then(|p| p.get("slow-timeout"))
            .unwrap_or_else(|| panic!("profile.{name}.slow-timeout missing"));
        let period = st
            .get("period")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("profile.{name}.slow-timeout.period missing"));
        assert_eq!(
            period, "60s",
            "profile.{name} slow-timeout period must equal the advertised 60s budget, got {period:?}"
        );
        let terminate_after = st
            .get("terminate-after")
            .and_then(|v| v.as_integer())
            .unwrap_or_else(|| panic!("profile.{name}.slow-timeout.terminate-after missing"));
        assert_eq!(
            terminate_after, 3,
            "profile.{name} slow-timeout terminate-after must stay 3"
        );
    }
}
