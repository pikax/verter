use std::path::Path;
use std::process::Command;

const PREFIX: &str = "server::server_tests::completion::svelte_progressive_component_completion_uses_committed_child_contract_";

fn output(command: &mut Command) -> String {
    let result = command.output().expect("start policy probe");
    assert!(
        result.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).expect("UTF-8 policy output")
}

fn lists_test(groups: &str, name: &str) -> bool {
    // Override descriptions echo filter expressions, including excluded names.
    // Only a test row has the complete test name as its first whitespace token.
    groups
        .lines()
        .any(|line| line.split_whitespace().next() == Some(name))
}

fn nextest(repo: &Path, metadata: &Path, profile: &str, config: &Path, args: &[&str]) -> String {
    output(
        Command::new("cargo-nextest")
            .current_dir(repo)
            .args(["nextest", "--color", "never", "--manifest-path"])
            .arg(repo.join("Cargo.toml"))
            .args(["--config-file"])
            .arg(config)
            .args(["--user-config-file", "none", "--profile", profile])
            .args(args)
            .arg("--cargo-metadata")
            .arg(metadata.join("cargo.json"))
            .arg("--binaries-metadata")
            .arg(metadata.join("binaries.json")),
    )
}

#[test]
fn mock_completion_inventory_and_effective_nextest_policy() {
    // Read libtest discovery, never source declarations: losing a test
    // attribute (or ignoring a variant) must fail the four-variant boundary.
    let binary = std::env::current_exe().expect("test binary");
    let inventory = output(Command::new(&binary).args(["--list", "--format", "terse"]));
    let discovered: Vec<_> = inventory
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    let mocks: Vec<_> = discovered
        .iter()
        .copied()
        .filter(|name| name.starts_with(PREFIX))
        .collect();
    let expected: Vec<_> = ["editor_tsserver", "none", "tsgo", "tsserver"]
        .map(|variant| format!("{PREFIX}{variant}"))
        .into_iter()
        .collect();
    assert_eq!(mocks, expected);
    let ignored = output(Command::new(&binary).args(["--list", "--ignored", "--format", "terse"]));
    assert!(!ignored.lines().any(|line| line.starts_with(PREFIX)));
    let controls: Vec<_> = ["_tsgo", "_tsserver"]
        .map(|suffix| {
            discovered
                .iter()
                .copied()
                .find(|name| {
                    name.starts_with("real_provider_tests::completion::") && name.ends_with(suffix)
                })
                .expect("compiled live-provider completion control")
        })
        .into_iter()
        .collect();

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo.join(".config/nextest.toml");
    let config_text = std::fs::read_to_string(&config_path).expect("nextest configuration");
    let config: toml::Value = toml::from_str(&config_text).expect("valid TOML");
    let core = output(Command::new("node").current_dir(&repo).args([
        "scripts/provider-ci.mjs",
        "filter",
        "core",
    ]));
    let core_mocks = format!("({}) & test({PREFIX})", core.trim());

    // Reuse this already-compiled libtest binary for native discovery. Supplying
    // listing-only build metadata avoids recursive Cargo builds and repeated
    // workspace metadata queries; nextest still discovers the tests itself.
    let temp = tempfile::tempdir().expect("policy metadata");
    let root = temp.path();
    let metadata = output(Command::new("cargo").current_dir(&repo).args([
        "metadata",
        "--locked",
        "--offline",
        "--format-version",
        "1",
    ]));
    let cargo: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    let package = cargo["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == env!("CARGO_PKG_NAME"))
        .expect("compiled test package");
    let deps = binary.parent().expect("Cargo deps directory");
    let build_dir = deps.parent().expect("Cargo profile directory");
    let target_dir = build_dir.parent().expect("Cargo target directory");
    let platform = output(Command::new("rustc").current_dir(&repo).args([
        "--print",
        "target-libdir",
        "--print",
        "host-tuple",
    ]));
    let mut platform = platform.lines();
    let libdir = platform.next().expect("Rust library directory");
    let triple = platform.next().expect("Rust host triple");
    let binaries = serde_json::json!({
        "rust-build-meta": {
            "target-directory": target_dir,
            "base-output-directories": [build_dir.file_name().unwrap().to_str().unwrap()],
            "non-test-binaries": {},
            "linked-paths": [],
            "platforms": {
                "host": {
                    "platform": { "triple": triple, "target-features": "unknown" },
                    "libdir": { "status": "available", "path": libdir },
                },
                "targets": [],
            },
        },
        "rust-binaries": {
            "verter_lsp": {
                "binary-id": "verter_lsp",
                "binary-name": "verter_lsp",
                "package-id": package["id"],
                "kind": "lib",
                "binary-path": binary,
                "build-platform": "target",
            },
        },
    });
    std::fs::write(root.join("cargo.json"), metadata).unwrap();
    std::fs::write(root.join("binaries.json"), binaries.to_string()).unwrap();
    let original = root.join("original.toml");
    std::fs::write(&original, &config_text).unwrap();

    // Project only timeout-bearing overrides onto a test-group setting. Both
    // properties use nextest's first matching override and profile inheritance.
    // Clearing the unrelated group settings lets show-config expose timeout
    // selection, including overrides whose filters never mention this matrix.
    // Keep slow-timeout itself intact so nextest validates its values as well.
    let mut probe = config.clone();
    for (_, profile) in probe["profile"].as_table_mut().unwrap().iter_mut() {
        if let Some(overrides) = profile
            .get_mut("overrides")
            .and_then(toml::Value::as_array_mut)
        {
            for item in overrides.iter_mut() {
                let item = item.as_table_mut().unwrap();
                item.remove("test-group");
                if item.contains_key("slow-timeout") {
                    item.insert("test-group".into(), "timeout-policy-probe".into());
                }
            }
            overrides.retain(|item| item.as_table().unwrap().len() > 1);
        }
    }
    probe["test-groups"].as_table_mut().unwrap().insert(
        "timeout-policy-probe".into(),
        toml::Value::Table(toml::Table::from_iter([("max-threads".into(), 1.into())])),
    );
    let projected = root.join("projected.toml");
    std::fs::write(&projected, toml::to_string(&probe).unwrap()).unwrap();

    for profile in ["default", "ci"] {
        // A group listing filtered through Core proves both selection and
        // serialization in one native probe. Every compiled mock must appear.
        let groups = nextest(
            &repo,
            root,
            profile,
            &original,
            &[
                "show-config",
                "test-groups",
                "--groups",
                "progressive-child-contract-completion",
                "-E",
                &core_mocks,
            ],
        );
        assert!(
            groups.contains("group: progressive-child-contract-completion (max threads = 1)"),
            "{groups}"
        );
        for name in &mocks {
            assert!(
                lists_test(&groups, name),
                "{profile}: {name} must remain serialized in Core: {groups}"
            );
        }
        let groups = nextest(
            &repo,
            root,
            profile,
            &projected,
            &[
                "show-config",
                "test-groups",
                "--groups",
                "timeout-policy-probe",
                "--run-ignored",
                "all",
                "-E",
                &format!(
                    "test({PREFIX}) | test(={}) | test(={})",
                    controls[0], controls[1]
                ),
            ],
        );
        for name in &mocks {
            assert!(
                !lists_test(&groups, name),
                "{profile}: {name} must inherit the base watchdog: {groups}"
            );
        }
        for name in &controls {
            assert!(
                lists_test(&groups, name),
                "{profile}: {name} retains its live-provider watchdog: {groups}"
            );
        }
    }
}
