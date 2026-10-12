//! Architecture guard: `.config/nextest.toml` must configure the speed gate
//! for BOTH the `default` and `ci` profiles: a test is flagged slow after 10s
//! and terminated, so failed, at 20s on CI (60s locally until the slow list
//! empties). A test that needs that
//! long in the debug build is a code problem (CLAUDE.md, Test Speed), so the
//! base limit is never raised; tests that predate the gate sit on the config's
//! shrinking "Known slow tests" list instead. This hermetic guard parses the
//! committed config and fails if the base limit drifts.

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
fn nextest_flags_a_test_at_10s_and_fails_it_at_20s_on_ci() {
    let path = repo_root().join(".config").join("nextest.toml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let cfg: toml::Value = toml::from_str(&text).expect("parse nextest.toml");
    let profile = cfg.get("profile").expect("[profile.*] table present");
    // CI fails a test at 20s. The local profile also runs the agents' gate on
    // loaded workers, so it fails at 60s until the "Known slow tests" list in
    // the config is empty; then it tightens to two periods as well.
    for (name, expected_terminate_after) in [("default", 6), ("ci", 2)] {
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
            "profile.{name} flags a test slow at 10s, got {period:?}"
        );
        let terminate_after = st
            .get("terminate-after")
            .and_then(|v| v.as_integer())
            .unwrap_or_else(|| panic!("profile.{name}.slow-timeout.terminate-after missing"));
        assert_eq!(
            terminate_after, expected_terminate_after,
            "profile.{name} terminates a test after {expected_terminate_after} 10s periods"
        );
    }
}

fn policy_output(command: &mut Command) -> String {
    let result = command.output().expect("start nextest policy probe");
    assert!(
        result.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).expect("UTF-8 policy output")
}

#[test]
fn every_compiled_corpus_chunk_has_the_twenty_second_nextest_deadline() {
    const PREFIX: &str = "cases::corpus_audit_tests::chunk_";
    const BASE_CONTROL: &str = "cases::corpus_audit_layout::shared_test_worker_pools_reuse_execution_substrate_not_host_state";
    let is_chunk =
        |suffix: &str| suffix.len() == 3 && suffix.bytes().all(|byte| byte.is_ascii_digit());
    let repo = repo_root();
    let binary = std::env::current_exe().expect("compiled integration binary");
    let inventory = policy_output(Command::new(&binary).args(["--list", "--format", "terse"]));
    let discovered: Vec<_> = inventory
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    let chunks: Vec<_> = discovered
        .iter()
        .copied()
        .filter(|name| {
            name.strip_prefix(PREFIX)
                .and_then(|rest| rest.split_once("::"))
                .is_some_and(|(suffix, _)| is_chunk(suffix))
        })
        .collect();
    let entry = std::fs::read_to_string(
        repo.join("crates/verter_session/tests/cases/corpus_audit_tests.rs"),
    )
    .expect("generated corpus entry point");
    let expected: Vec<_> = entry
        .lines()
        .filter_map(|line| line.strip_prefix("mod chunk_")?.strip_suffix(';'))
        .filter(|suffix| is_chunk(suffix))
        .map(|suffix| {
            format!("{PREFIX}{suffix}::corpus_audit_chunk_{suffix}_produces_audit_records_or_documents_skips")
        })
        .collect();
    assert!(!expected.is_empty(), "corpus inventory must not be empty");
    assert_eq!(chunks, expected, "every generated chunk must be compiled");
    assert!(discovered.contains(&BASE_CONTROL), "compiled base control");
    let ignored =
        policy_output(Command::new(&binary).args(["--list", "--ignored", "--format", "terse"]));
    assert!(
        !ignored.lines().any(|line| line.starts_with(PREFIX)),
        "corpus chunks must not be ignored"
    );

    // Listing-only metadata reuses the running binary; no nested test build.
    // Archive consumers have no registry cache. Manifest-only metadata supplies
    // the workspace package identities used by the policy's package selectors.
    let temp = tempfile::tempdir().expect("policy probe directory");
    let cargo_home = temp.path().join("cargo-home");
    std::fs::create_dir(&cargo_home).expect("empty Cargo home for archive consumer control");
    let metadata = policy_output(
        Command::new("cargo")
            .current_dir(&repo)
            .env("CARGO_HOME", &cargo_home)
            .args([
                "metadata",
                "--no-deps",
                "--locked",
                "--offline",
                "--format-version",
                "1",
            ]),
    );
    let cargo: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    let package = cargo["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == env!("CARGO_PKG_NAME"))
        .expect("compiled test package");
    let build_dir = binary.parent().unwrap().parent().unwrap();
    let platform = policy_output(Command::new("rustc").current_dir(&repo).args([
        "--print",
        "target-libdir",
        "--print",
        "host-tuple",
    ]));
    let mut platform = platform.lines();
    let libdir = platform.next().unwrap();
    let triple = platform.next().unwrap();
    let binaries = serde_json::json!({
        "rust-build-meta": {
            "target-directory": build_dir.parent().unwrap(),
            "base-output-directories": [build_dir.file_name().unwrap().to_str().unwrap()],
            "non-test-binaries": {}, "linked-paths": [],
            "platforms": {
                "host": {
                    "platform": { "triple": triple, "target-features": "unknown" },
                    "libdir": { "status": "available", "path": libdir },
                },
                "targets": [],
            },
        },
        "rust-binaries": {
            "verter_session::main": {
                "binary-id": "verter_session::main", "binary-name": "main",
                "package-id": package["id"], "kind": "test",
                "binary-path": binary, "build-platform": "target",
            },
        },
    });
    std::fs::write(temp.path().join("cargo.json"), metadata).unwrap();
    std::fs::write(temp.path().join("binaries.json"), binaries.to_string()).unwrap();

    let mut config: toml::Value =
        toml::from_str(&std::fs::read_to_string(repo.join(".config/nextest.toml")).unwrap())
            .unwrap();
    let speed: toml::Value = toml::from_str("period = '10s'\nterminate-after = 2").unwrap();
    let group = |timeout: &toml::Value| {
        if timeout == &speed {
            "corpus-speed"
        } else {
            "other-timeout"
        }
    };
    // Project slow-timeout onto groups, which show-config exposes. Both settings
    // use nextest's first matching override and profile inheritance. Preserve
    // all selectors/order and timeout values, including broad future overrides.
    for (_, profile) in config["profile"].as_table_mut().unwrap().iter_mut() {
        let profile = profile.as_table_mut().unwrap();
        if let Some(overrides) = profile
            .get_mut("overrides")
            .and_then(toml::Value::as_array_mut)
        {
            for item in overrides.iter_mut() {
                let item = item.as_table_mut().unwrap();
                item.remove("test-group");
                if let Some(timeout) = item.get("slow-timeout") {
                    item.insert("test-group".into(), group(timeout).into());
                }
            }
            overrides.retain(|item| item.as_table().unwrap().len() > 1);
        }
    }
    for name in ["corpus-speed", "other-timeout"] {
        config["test-groups"].as_table_mut().unwrap().insert(
            name.into(),
            toml::Value::Table(toml::Table::from_iter([("max-threads".into(), 1.into())])),
        );
    }
    let projected = temp.path().join("projected.toml");
    std::fs::write(&projected, toml::to_string(&config).unwrap()).unwrap();
    for profile in ["default", "ci"] {
        let groups = policy_output(
            Command::new("cargo-nextest")
                .current_dir(&repo)
                .env("CARGO_HOME", &cargo_home)
                .args(["nextest", "--color", "never", "--config-file"])
                .arg(&projected)
                .args([
                    "--user-config-file",
                    "none",
                    "--profile",
                    profile,
                    "show-config",
                    "test-groups",
                    "--groups",
                    "corpus-speed",
                    "--run-ignored",
                    "all",
                    "-E",
                    &format!("test({PREFIX}) | test(={BASE_CONTROL})"),
                    "--cargo-metadata",
                ])
                .arg(temp.path().join("cargo.json"))
                .arg("--binaries-metadata")
                .arg(temp.path().join("binaries.json")),
        );
        let has_speed = |name: &str| {
            groups
                .lines()
                .any(|line| line.split_whitespace().next() == Some(name))
        };
        for chunk in &chunks {
            assert!(
                has_speed(chunk),
                "{profile}: {chunk} must have a 20-second deadline: {groups}"
            );
        }
        assert!(
            !has_speed(BASE_CONTROL),
            "{profile}: unrelated control must inherit the base policy: {groups}"
        );
    }
}
