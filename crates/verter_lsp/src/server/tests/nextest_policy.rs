use std::collections::BTreeMap;
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

#[derive(Default)]
struct TestModule {
    children: BTreeMap<String, TestModule>,
    tests: Vec<String>,
}

impl TestModule {
    fn insert(&mut self, name: &str) {
        if let Some((module, rest)) = name.split_once("::") {
            self.children
                .entry(module.to_string())
                .or_default()
                .insert(rest);
        } else {
            self.tests.push(name.to_string());
        }
    }

    fn source(&self) -> String {
        let mut source = String::new();
        for name in &self.tests {
            source.push_str(&format!(
                "#[test] fn {name}() {{ panic!(\"listing fixture must never execute\") }}\n"
            ));
        }
        for (name, module) in &self.children {
            source.push_str(&format!("mod {name} {{ {} }}\n", module.source()));
        }
        source
    }
}

fn nextest(root: &Path, profile: &str, config: &Path, args: &[&str]) -> String {
    output(
        Command::new("cargo")
            .current_dir(root)
            .args(["nextest", "--color", "never", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .args(["--config-file"])
            .arg(config)
            .args(["--profile", profile])
            .args(args),
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

    // Small compiled listing fixtures retain the discovered package and test
    // identities. They exercise nextest's real parser and override resolution
    // without rebuilding this library recursively from inside its own test.
    let temp = tempfile::tempdir().expect("policy workspace");
    let root = temp.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'verter_lsp'\nversion = '0.0.0'\nedition = '2024'\n[workspace]\nmembers = ['packages/*']\ndefault-members = ['.']\n",
    )
    .unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    let metadata = output(Command::new("cargo").current_dir(&repo).args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ]));
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    // Nextest validates package selectors against Cargo's entire workspace
    // graph, including packages whose binaries this probe does not select.
    for package in metadata["packages"].as_array().unwrap() {
        let name = package["name"].as_str().unwrap();
        if name == "verter_lsp" {
            continue;
        }
        let package_root = root.join("packages").join(name);
        std::fs::create_dir_all(package_root.join("src")).unwrap();
        std::fs::write(
            package_root.join("Cargo.toml"),
            format!("[package]\nname = '{name}'\nversion = '0.0.0'\nedition = '2024'\n"),
        )
        .unwrap();
        std::fs::write(package_root.join("src/lib.rs"), "").unwrap();
    }
    let mut module = TestModule::default();
    for name in mocks.iter().chain(controls.iter()) {
        module.insert(name);
    }
    std::fs::write(root.join("src/lib.rs"), module.source()).unwrap();
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
        let listing = nextest(
            root,
            profile,
            &original,
            &[
                "list",
                "--message-format",
                "json",
                "--run-ignored",
                "all",
                "-E",
                &core_mocks,
            ],
        );
        let listing: serde_json::Value =
            serde_json::from_str(&listing).expect("nextest discovery JSON");
        let selected: Vec<_> = listing["rust-suites"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|suite| suite["testcases"].as_object().unwrap().iter())
            .filter(|(_, test)| test["filter-match"]["status"] == "matches")
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(selected, mocks, "{profile}: every mock stays in Core");
        let groups = nextest(
            root,
            profile,
            &original,
            &[
                "show-config",
                "test-groups",
                "--groups",
                "progressive-child-contract-completion",
                "-E",
                &format!("test({PREFIX})"),
            ],
        );
        assert!(
            groups.contains("group: progressive-child-contract-completion (max threads = 1)"),
            "{groups}"
        );
        for name in &mocks {
            assert!(
                groups.contains(name),
                "{profile}: {name} must remain serialized: {groups}"
            );
        }
        let groups = nextest(
            root,
            profile,
            &projected,
            &[
                "show-config",
                "test-groups",
                "--groups",
                "timeout-policy-probe",
            ],
        );
        for name in &mocks {
            assert!(
                !groups.contains(name),
                "{profile}: {name} must inherit the base watchdog: {groups}"
            );
        }
        for name in &controls {
            assert!(
                groups.contains(name),
                "{profile}: {name} retains its live-provider watchdog: {groups}"
            );
        }
    }
}
