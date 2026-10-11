use super::{session::*, LibMode, Sink};
use std::path::Path;

const CHILD_OUT: &str = "VERTER_BENCH_MEMORY_CHILD_OUT";

pub(super) fn before_step(index: usize) {
    if index != 2 || std::env::var_os(CHILD_OUT).is_none() {
        return;
    }
    let mut held = Vec::new();
    loop {
        let mut chunk = Vec::new();
        if chunk.try_reserve_exact(8 * 1024 * 1024).is_err() {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        chunk.resize(8 * 1024 * 1024, 1u8);
        std::hint::black_box(&chunk);
        held.push(chunk);
    }
}

fn job(dir: &Path) -> SessionJob {
    std::fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"noLib":true}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("lib.d.ts"), "").unwrap();
    std::fs::write(
        dir.join("input.ts"),
        "export type Init = 0; export type Answer = 42;",
    )
    .unwrap();
    SessionJob {
        schema: SESSION_JOB_SCHEMA,
        dir: dir.into(),
        tsconfig: "tsconfig.json".into(),
        lib: "lib.d.ts".into(),
        lib_mode: LibMode::RootFile,
        files: vec!["input.ts".into()],
        init_file: "input.ts".into(),
        init_alias: "Init".into(),
        observability: false,
        steps: (0..4)
            .map(|_| Step::Demand {
                concurrent: false,
                requests: vec![Demand {
                    file: "input.ts".into(),
                    alias: "Answer".into(),
                }],
            })
            .collect(),
    }
}

#[test]
fn complete_session_journal_contains_each_real_answer_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("session.json");
    let result = run_session(&job(dir.path()), &Sink::new(&out)).unwrap();
    assert_eq!(result.steps.len(), 4);
    let journal = std::fs::read_to_string(dir.path().join("session.json.steps.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = journal
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(entries[0]["stage"], "partial");
    assert_eq!(entries.len(), 5);
    for (index, entry) in entries[1..].iter().enumerate() {
        assert_eq!(entry["index"], index);
        assert_eq!(entry["step"]["requests"][0]["observation"]["text"], "42");
        assert_eq!(entry["step"]["requests"][0]["outcome"]["kind"], "value");
    }
}

#[test]
#[ignore = "fixture subprocess; launched by the supervised memory-kill test"]
fn session_memory_child() {
    let out = std::env::var_os(CHILD_OUT).expect("child output path");
    let out = Path::new(&out);
    run_session(&job(out.parent().unwrap()), &Sink::new(out)).unwrap();
    panic!("the memory cap must kill the third step");
}

// The library API establishes a Windows job without needing a second
// executable. Unix watchdogs require the standalone supervisor entry point.
#[test]
#[cfg(windows)]
fn memory_kill_keeps_two_answers_and_the_running_step_phase() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("session.json");
    let report = dir.path().join("supervisor.json");
    let exe = std::env::current_exe().unwrap();
    let args = vec![
        "run".into(),
        "--mem-mb".into(),
        "256".into(),
        "--timeout-ms".into(),
        "30000".into(),
        "--out".into(),
        report.as_os_str().to_owned(),
        "--env".into(),
        format!("{CHILD_OUT}={}", out.display()).into(),
        "--".into(),
        exe.into_os_string(),
        "--exact".into(),
        "semantic_perf::session_tests::session_memory_child".into(),
        "--ignored".into(),
        "--nocapture".into(),
    ];
    let spec = verter_supervise::cli::parse(args).unwrap();
    assert_eq!(verter_supervise::run(&spec), 137);
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    assert_eq!(report["killedBy"], "memory");
    assert_eq!(report["errors"], serde_json::json!([]));
    assert!(!out.exists());
    let journal = std::fs::read_to_string(dir.path().join("session.json.steps.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = journal
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(entries.len(), 3);
    for (index, entry) in entries[1..].iter().enumerate() {
        assert_eq!(entry["index"], index);
        assert_eq!(entry["step"]["requests"][0]["observation"]["text"], "42");
        assert!(entry["step"]["wallMicros"].as_u64().is_some());
    }
    let phase: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("session.json.phase")).unwrap(),
    )
    .unwrap();
    assert_eq!(phase["phase"], "step-2");
}
