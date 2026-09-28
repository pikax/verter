//! Architecture guard: `.config/nextest.toml` must configure the advertised
//! test-duration budget for BOTH the `default` and `ci` profiles: a test is
//! flagged slow after 10s and terminated after two periods (20s, the budget
//! with headroom for slower CI runners). A longer period lets a test past the
//! 10s budget pass unnoticed; a test that needs longer is named in an
//! override with its reason, never covered by a looser profile. This hermetic
//! guard parses the committed config and fails if the effective budget
//! drifts.

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
fn nextest_slow_timeout_period_is_10s_for_both_profiles() {
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
            period, "10s",
            "profile.{name} slow-timeout period must equal the advertised 10s budget, got {period:?}"
        );
        let terminate_after = st
            .get("terminate-after")
            .and_then(|v| v.as_integer())
            .unwrap_or_else(|| panic!("profile.{name}.slow-timeout.terminate-after missing"));
        assert_eq!(
            terminate_after, 2,
            "profile.{name} slow-timeout terminate-after must stay 2"
        );
    }
}
