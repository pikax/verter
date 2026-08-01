//! End-to-end: a JavaScript SFC is typechecked as JavaScript, not TypeScript.
//!
//! Drives the REAL producer — the `verter-tsc` binary, so `generate_all_tsx`
//! and `generate_public_api_stubs` compose the generated-root names and the
//! in-memory `--api` overlay is what the engine actually sees — against a
//! vendored fixture project holding both JavaScript and TypeScript SFCs, in
//! both `<script setup>` and Options-API form.
//!
//! TypeScript decides what to typecheck from a file's extension/ScriptKind:
//! `.tsx`/`.ts` are ALWAYS checked (so `strict`/`noImplicitAny` reports TS7006
//! on every untyped parameter), while `.jsx`/`.js` are checked only under
//! `checkJs`. The CLI used to name every generated root TypeScript, so a JS SFC
//! was mislabelled and produced a TS7006 flood that neither `vue-tsc` (with
//! `checkJs` off) nor Verter's own LSP produces for the same file — through TWO
//! roots: the validation carrier and the public-API stub. The fix is the LABEL
//! — not a `checkJs` switch and not a diagnostic filter — so `checkJs: true`
//! must still report real JavaScript errors.
//!
//! Prerequisites are FAIL-CLOSED, not skip-closed. A genuinely absent engine
//! (no candidate found anywhere) is the ONE loud, distinguishable skip; an
//! engine that WAS found and failed validation, a missing fixture dependency, a
//! failed junction/symlink, and a wedged run are FAILURES with their reason.
//! `Ok`-collapsing any of those would turn "broken toolchain" into "clean run".
//!
//! # Why the harness is a closed module
//!
//! Those prerequisite rules are only worth their green if the ORACLE is bound
//! to them, and a test that drives a helper directly proves nothing about who
//! calls it: reverting the oracle's own `resolve_gated_engine(…)` line to
//! `.ok()`, or its `run_verter_tsc(…)` line to `Command::output()`, leaves a
//! helper-level test perfectly green while the oracle silently skips a broken
//! toolchain or hangs on a wedged child. Twice-reviewed, twice unresolved.
//!
//! So the binding is STRUCTURAL, not asserted. Everything the oracle needs to
//! run the binary lives inside [`harness`] behind types with private fields:
//! the only `GatedEngine` in existence comes out of the skip-vs-fail gate, the
//! only `GatedCommand` comes from that engine, the only `GatedRun` comes out of
//! the deadline-bounded runner, and the only `Diag` comes out of a `GatedRun`.
//! `Result::ok()` yields a `Resolution`, and `Command::output()` yields an
//! `Output`; neither is any of those types, so neither revert COMPILES. The
//! module's own `#[test]`s cover what the types cannot — that the gate
//! separates absent from broken, and that the runner kills a wedged tree —
//! and they live inside the module precisely because building the closed types
//! by hand is what the oracle must not be able to do.

use std::path::{Path, PathBuf};

use harness::{Diag, GatedRun, RUN_DEADLINE};

/// The closed seam: engine prerequisite gate + deadline-bounded runner.
///
/// Nothing here is reachable except through the entry points, and the types
/// they produce cannot be built any other way — see the module docs above.
mod harness {
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// Hard kill-deadline for one `verter-tsc` invocation.
    ///
    /// This is a DEADLINE, not a performance assertion: nothing here asserts the
    /// run finished in under N milliseconds (that assertion family is a
    /// load-flake generator). It exists so a wedged child is killed and reported
    /// HERE, with its reason, instead of riding until the outer gate kills the
    /// whole job.
    ///
    /// It must be STRICTLY BELOW its parent killer or it never applies: the
    /// canonical gate runs under `cargo nextest`, whose profile
    /// (`.config/nextest.toml`) terminates a test after
    /// `slow-timeout.period x terminate-after`. The oracle makes TWO
    /// invocations, so the budget is `2 x 60s` worst case, and
    /// `the_run_deadline_is_strictly_below_the_nextest_termination_deadline`
    /// checks both against the profile that will actually run.
    pub const RUN_DEADLINE: Duration = Duration::from_secs(60);

    /// The marker a skip line carries so a skipped run is distinguishable from a
    /// run that executed. Only ONE condition may emit it: no tsgo candidate
    /// exists anywhere on this machine.
    const SKIP_MARKER: &str = "SKIP(js_carrier_script_kind): no tsgo engine candidate exists";

    /// One parsed engine diagnostic.
    ///
    /// Fields are private and there is no constructor: the ONLY way to obtain a
    /// `Diag` is [`GatedRun::diags`], which means the only way to obtain one is
    /// to have run the binary through [`run_verter_tsc`].
    #[derive(Debug, Clone)]
    pub struct Diag {
        file: String,
        ts_code: u32,
    }

    impl Diag {
        /// The reported file path, as the engine spelled it.
        #[must_use]
        pub fn file(&self) -> &str {
            &self.file
        }

        /// The `TS<code>` number.
        #[must_use]
        pub fn ts_code(&self) -> u32 {
            self.ts_code
        }
    }

    /// A tsgo engine that PASSED the prerequisite gate.
    ///
    /// Unconstructible outside this module, and the only thing
    /// [`verter_tsc_command`] accepts — so an oracle cannot reach the binary
    /// with an engine the gate never judged.
    #[derive(Debug)]
    pub struct GatedEngine(PathBuf);

    /// A `verter-tsc` invocation built from a [`GatedEngine`], runnable only
    /// through [`run_verter_tsc`].
    #[derive(Debug)]
    pub struct GatedCommand(Command);

    /// The result of a run that COMPLETED inside its deadline.
    ///
    /// A run that blew its deadline panics instead of producing one of these,
    /// so a `GatedRun` in hand is proof the bound held.
    #[derive(Debug)]
    pub struct GatedRun {
        diags: Vec<Diag>,
        stdout: String,
    }

    impl GatedRun {
        /// Every parsed engine diagnostic from the run.
        #[must_use]
        pub fn diags(&self) -> &[Diag] {
            &self.diags
        }

        /// The raw stdout, for assertions about Verter-native output that
        /// carries no `TS<code>` (and so never parses into a [`Diag`]).
        #[must_use]
        pub fn stdout(&self) -> &str {
            &self.stdout
        }
    }

    /// `path(line,col): error TSxxxx: message`
    fn parse_diagnostics(output: &str) -> Vec<Diag> {
        output
            .lines()
            .filter_map(|line| {
                let paren = line.find('(')?;
                let close = line[paren..].find("): ")? + paren;
                let file = line[..paren].trim().to_string();
                let rest = &line[close + 3..];
                let code_start = rest.find("error TS")? + "error TS".len();
                let code_rest = &rest[code_start..];
                let code_end = code_rest.find(':')?;
                let ts_code = code_rest[..code_end].parse().ok()?;
                Some(Diag { file, ts_code })
            })
            .collect()
    }

    /// What the engine-resolution outcome means for this fixture's
    /// prerequisites.
    #[derive(Debug)]
    enum EnginePrereq {
        /// A validated engine — run the oracle.
        Ready(PathBuf),
        /// The resolver enumerated NO candidate at all. The one legitimate skip.
        Absent(String),
        /// A candidate WAS found and failed validation (wrong version, failed
        /// `--version` probe, failed capability handshake) — a broken toolchain,
        /// which is not an absent one.
        Broken(String),
    }

    /// The gated `--api` engine, decided from a resolution OUTCOME.
    ///
    /// `Result::ok` maps [`EnginePrereq::Broken`] and [`EnginePrereq::Absent`]
    /// to the same `None`, and an oracle that skips on "broken" reports a broken
    /// toolchain as a clean run. That revert cannot be made at the call site
    /// either: `.ok()` produces a `Resolution`, and the only thing that reaches
    /// the binary is a [`GatedEngine`], which only this function mints.
    ///
    /// `None` means GENUINELY ABSENT — the one loud, distinguishable skip. A
    /// found-but-rejected candidate PANICS.
    pub fn resolve_gated_engine(
        outcome: Result<
            verter_tsgo_api::toolchain::discovery::Resolution,
            verter_tsgo_api::toolchain::discovery::ResolveError,
        >,
    ) -> Option<GatedEngine> {
        use verter_tsgo_api::toolchain::discovery::ResolveError;

        let verdict = match outcome {
            Ok(resolution) => EnginePrereq::Ready(resolution.path),
            Err(ResolveError::NoUsableCandidate {
                rejections, notes, ..
            }) if rejections.is_empty() => EnginePrereq::Absent(format!("tier notes: {notes:?}")),
            Err(ResolveError::NoUsableCandidate { rejections, .. }) => EnginePrereq::Broken(
                rejections
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n  - "),
            ),
            Err(other) => EnginePrereq::Broken(other.to_string()),
        };

        match verdict {
            EnginePrereq::Ready(path) => Some(GatedEngine(path)),
            EnginePrereq::Absent(notes) => {
                eprintln!(
                    "{SKIP_MARKER} (set VERTER_TSGO_BIN or run `pnpm install \
                     --frozen-lockfile` in the workspace). {notes}"
                );
                None
            }
            EnginePrereq::Broken(detail) => panic!(
                "a tsgo engine WAS found and failed validation — a broken toolchain is not \
                 an absent one, and must never be skipped past:\n  - {detail}"
            ),
        }
    }

    /// Resolve the gated `--api` engine through the shared capability-validated
    /// toolchain resolver against the workspace root (where
    /// `pnpm install --frozen-lockfile` installs the pinned `typescript@7.0.2`).
    pub fn gated_engine_resolution() -> Result<
        verter_tsgo_api::toolchain::discovery::Resolution,
        verter_tsgo_api::toolchain::discovery::ResolveError,
    > {
        let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
            verter_tsgo_api::toolchain::validation::Capability::Lsp,
            Some(super::workspace_root()),
        );
        verter_tsgo_api::toolchain::discovery::resolve_blocking(&request)
    }

    /// The `verter-tsc` invocation for one of the fixture's tsconfigs.
    pub fn verter_tsc_command(
        project: &Path,
        engine: &GatedEngine,
        tsconfig: &str,
    ) -> GatedCommand {
        let mut command = Command::new(env!("CARGO_BIN_EXE_verter-tsc"));
        command
            .env("VERTER_TSGO_BIN", &engine.0)
            .arg("--noEmit")
            .arg("-p")
            .arg(project.join(tsconfig));
        GatedCommand(command)
    }

    /// The EMITTING `verter-tsc` invocation: `--declaration` into
    /// `declaration_dir`, listing every artifact it writes.
    ///
    /// `--noEmit` is deliberately absent. A "nothing was generated for this
    /// source" claim checked only under `--noEmit` proves nothing about
    /// generation — that flag suppresses emission for every file, refused or
    /// not, so a leaked companion would be invisible.
    pub fn verter_tsc_emit_command(
        project: &Path,
        engine: &GatedEngine,
        tsconfig: &str,
        declaration_dir: &Path,
    ) -> GatedCommand {
        let mut command = Command::new(env!("CARGO_BIN_EXE_verter-tsc"));
        command
            .env("VERTER_TSGO_BIN", &engine.0)
            .arg("--declaration")
            .arg("--declarationDir")
            .arg(declaration_dir)
            .arg("--listEmittedFiles")
            .arg("-p")
            .arg(project.join(tsconfig));
        GatedCommand(command)
    }

    /// Run `command` to completion under a hard kill-deadline and return the
    /// parsed diagnostics plus the raw stdout.
    ///
    /// A child that has not exited by `deadline` has its whole PROCESS TREE
    /// killed and the call PANICS naming the deadline. `Command::output()`
    /// cannot do this: it blocks forever, so a wedged child rides until the
    /// outer gate kills the job and the failure is reported as a timeout of
    /// everything rather than of this. That revert cannot be made at the call
    /// site either: `Command::output()` produces an `Output`, and every
    /// assertion downstream consumes a [`GatedRun`], which only this function
    /// mints. Nothing here asserts how long a healthy run takes.
    pub fn run_verter_tsc(command: GatedCommand, label: &str, deadline: Duration) -> GatedRun {
        let mut command = command.0;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        verter_tsgo_api::process::configure_tree_spawn_std(&mut command);

        let mut child = command
            .spawn()
            .unwrap_or_else(|e| panic!("failed to execute {label}: {e}"));
        let tree = verter_tsgo_api::process::TreeKill::arm(child.id());

        let mut out_pipe = child.stdout.take().expect("stdout was piped");
        let mut err_pipe = child.stderr.take().expect("stderr was piped");
        let out_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out_pipe.read_to_end(&mut buf);
            buf
        });
        let err_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err_pipe.read_to_end(&mut buf);
            buf
        });

        let hard_stop = Instant::now() + deadline;
        let mut blew_deadline = false;
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) => {
                    if Instant::now() >= hard_stop {
                        blew_deadline = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => {
                    tree.kill_tree();
                    let _ = child.wait();
                    panic!("failed to wait for {label}: {e}");
                }
            }
        }

        // Kill the tree BEFORE joining, on BOTH exits. The direct child exiting
        // is not the same as the pipes closing: a grandchild that inherited them
        // keeps the readers from ever reaching EOF, and `join()` has no timeout —
        // the test would hang holding `tree`, so even `TreeKill::drop` never
        // runs. The group is killed exactly once here; on the healthy path it is
        // already empty (ESRCH, a no-op) and the readers are already at EOF.
        tree.kill_tree();
        let _ = child.wait();

        let stdout =
            String::from_utf8_lossy(&out_reader.join().expect("stdout reader")).into_owned();
        let stderr =
            String::from_utf8_lossy(&err_reader.join().expect("stderr reader")).into_owned();

        assert!(
            !blew_deadline,
            "{label} did not finish within {deadline:?}; its process tree was killed. \
             A wedged run is a failure, not a slow pass.\nstderr:\n{stderr}"
        );

        eprintln!("=== {label} STDERR ===\n{stderr}");
        eprintln!("=== {label} STDOUT ===\n{stdout}");
        GatedRun {
            diags: parse_diagnostics(&stdout),
            stdout,
        }
    }

    // ── Harness integrity ───────────────────────────────────────────────
    //
    // The type-state above binds the oracle to this seam — the reverts it
    // exists to prevent no longer compile. These two cover what types cannot:
    // that the gate's two failure arms really are different, and that the
    // runner really does kill a wedged tree. They live INSIDE the module
    // because building `GatedCommand` by hand is exactly the escape the oracle
    // must not have.

    /// Recover a panic payload as a string, or `None` when the call returned.
    fn panic_text<T>(outcome: std::thread::Result<T>) -> Option<String> {
        outcome.err().map(|payload| {
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default()
        })
    }

    /// DISCRIMINATING: the prerequisite gate separates "no engine exists" (skip)
    /// from "an engine exists and is broken" (fail).
    ///
    /// `Result::ok` maps both arms to `None`: a runner with a broken tsgo — a
    /// below-floor version, a binary that fails the capability handshake — would
    /// silently skip the entire oracle and the suite would go green having
    /// proved nothing.
    #[test]
    fn engine_prerequisite_gate_separates_an_absent_engine_from_a_broken_one() {
        use verter_tsgo_api::toolchain::discovery::{
            Candidate, CandidateRejection, Provenance, ResolveError,
        };
        use verter_tsgo_api::toolchain::validation::{Capability, RejectionReason};

        // Absent: the resolver walked every tier and enumerated NOTHING. The
        // gate returns `None`, which is what makes the oracle `return` — a skip.
        let absent = resolve_gated_engine(Err(ResolveError::NoUsableCandidate {
            rejections: Vec::new(),
            notes: vec!["project-local node_modules not found".to_string()],
            requirement: Capability::Lsp,
        }));
        assert!(
            absent.is_none(),
            "no candidate at all is the one legitimate skip: {absent:?}"
        );

        // Broken: a candidate WAS found and failed validation. This is the arm
        // `.ok()` erases — it must ABORT the run, never skip it.
        let broken = panic_text(std::panic::catch_unwind(|| {
            resolve_gated_engine(Err(ResolveError::NoUsableCandidate {
                rejections: vec![CandidateRejection {
                    candidate: Candidate {
                        path: PathBuf::from("/usr/local/bin/tsgo"),
                        provenance: Provenance::SharedPath,
                    },
                    reason: RejectionReason::VersionProbeFailed {
                        detail: "exit status 1".to_string(),
                    },
                }],
                notes: Vec::new(),
                requirement: Capability::Lsp,
            }))
        }))
        .expect(
            "a found-but-rejected candidate is a BROKEN toolchain, never an absent one — \
             it must fail the run, not return `None` and skip it",
        );
        assert!(
            broken.contains("/usr/local/bin/tsgo"),
            "the failure must name the candidate it rejected: {broken}"
        );

        // And the success arm still hands the path through.
        let ready = resolve_gated_engine(Ok(verter_tsgo_api::toolchain::discovery::Resolution {
            path: PathBuf::from("/opt/tsgo"),
            provenance: Provenance::ProjectLocal,
            version: verter_tsgo_api::toolchain::policy::TsgoVersion::parse("7.0.2").unwrap(),
            rejections: Vec::new(),
        }))
        .expect("a validated engine must be handed through");
        assert_eq!(
            ready.0,
            PathBuf::from("/opt/tsgo"),
            "a validated engine must be handed through unchanged"
        );
    }

    /// DISCRIMINATING: the runner KILLS a wedged child's process tree and FAILS,
    /// instead of blocking until the outer gate kills the whole job.
    ///
    /// Its child spawns a grandchild that inherits the pipes and then hangs
    /// forever, the exact shape `Command::output()` cannot survive (it waits on
    /// an EOF that never comes) and the exact shape that deadlocks a runner
    /// which joins its pipe readers before killing the tree. Nothing here
    /// asserts how long a healthy run takes; the only timing involved is the
    /// deadline itself, and the outer bound below exists so a runner that DOES
    /// block fails this test in seconds rather than hanging the gate.
    ///
    /// **Unix-only, deliberately.** The wedge fixture is a POSIX shell script
    /// (`sleep &` + `wait`), the pid bookkeeping is `$!`, and the liveness probe
    /// is a signal-0 `kill`. A faithful Windows equivalent is not a translation
    /// of this script — it needs a job-object-aware wedge and a different
    /// liveness oracle — and writing one that has never been executed on Windows
    /// would assert nothing while looking like coverage. The production kill
    /// path it exercises, `verter_tsgo_api::process::TreeKill`, is owned and
    /// tested by that crate on both platforms; the SIBLING wedged-engine
    /// discrimination in `verter_tsc::checker`
    /// (`invoke_checker_kills_a_wedged_engine_tree_within_the_bound`) is
    /// `#[cfg(unix)]` for the same reason. What is lost on Windows is this
    /// harness's own discrimination, not the shipped behaviour.
    #[cfg(unix)]
    #[test]
    fn the_runner_kills_a_wedged_child_tree_instead_of_blocking_forever() {
        use verter_tsgo_api::process::process_alive;

        let temp = tempfile::TempDir::new().unwrap();
        let pid_file = temp.path().join("grandchild.pid");
        let script = temp.path().join("wedged");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 600 &\necho $! > \"{}\"\nwait\n",
                pid_file.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        // Run it OFF this thread with a liveness bound. A runner that blocks —
        // on `Command::output()`, or on joining a pipe reader a live grandchild
        // keeps open — otherwise hangs until the gate's own killer, reporting a
        // timeout of everything instead of a failure of this. The bound is
        // liveness only: it is six times the runner's own deadline, so it can
        // only be reached by a runner that does not terminate at all.
        //
        // The runner's deadline here has to outlast the FIXTURE's startup, not
        // just the wedge: `/bin/sh` must run, fork `sleep 600`, and record the
        // pid before the kill lands, and under the gate's test parallelism that
        // is not instant. Ten seconds against a 600s sleep still proves the
        // bound fires.
        let (tx, rx) = std::sync::mpsc::channel();
        let wedged = script.clone();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(|| {
                run_verter_tsc(
                    GatedCommand(Command::new(&wedged)),
                    "wedged-fixture",
                    Duration::from_secs(10),
                )
            });
            let _ = tx.send(panic_text(outcome));
        });

        let panic_message = rx
            .recv_timeout(Duration::from_secs(60))
            .expect(
                "the runner never returned: a wedged child must be killed and reported \
                 HERE, not left to block until the outer gate kills the job",
            )
            .expect("a wedged child must FAIL the run, never return a clean empty output");
        assert!(
            panic_message.contains("did not finish within"),
            "the failure must name the deadline it blew: {panic_message}"
        );

        // Negative: the tree really was killed — a grandchild left alive would
        // mean the "bound" only abandoned the child and leaked the real process.
        // Poll for the record: the fixture writes it from a shell that may still
        // be scheduling when the bound fires.
        let pid_deadline = Instant::now() + Duration::from_secs(5);
        let recorded = loop {
            match std::fs::read_to_string(&pid_file) {
                Ok(text) if !text.trim().is_empty() => break text,
                other => {
                    assert!(
                        Instant::now() < pid_deadline,
                        "the wedged fixture never recorded its grandchild pid: {other:?}"
                    );
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        };
        let grandchild: u32 = recorded.trim().parse().expect("a numeric pid");
        let deadline = Instant::now() + Duration::from_secs(10);
        while process_alive(grandchild) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !process_alive(grandchild),
            "the wedged child's grandchild ({grandchild}) survived the bound — the deadline \
             killed the direct child only and leaked the process holding the pipes"
        );
    }
}

// ── Diagnostic filters ──────────────────────────────────────────────────

fn for_file<'a>(diags: &'a [Diag], suffix: &str) -> Vec<&'a Diag> {
    diags
        .iter()
        .filter(|d| d.file().replace('\\', "/").ends_with(suffix))
        .collect()
}

/// Every diagnostic whose reported path mentions `stem` at all — the `.vue`
/// source AND any generated root derived from it (`{stem}_<hash>.jsx`,
/// `{stem}_<hash>.vue.js`, …). Used for the "this source contributes NOTHING"
/// assertions, which a `/src/X.vue`-only filter would report vacuously while a
/// generated root quietly carried the flood.
fn mentioning<'a>(diags: &'a [Diag], stem: &str) -> Vec<&'a Diag> {
    diags
        .iter()
        .filter(|d| {
            Path::new(&d.file().replace('\\', "/"))
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(stem))
        })
        .collect()
}

fn codes(diags: &[&Diag]) -> Vec<u32> {
    diags.iter().map(|d| d.ts_code()).collect()
}

// ── Setup ───────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("could not find workspace root")
        .to_path_buf()
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join("fixtures")
        .join("js_carrier")
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Link `src` into `dest`, returning the failure reason instead of swallowing
/// it — a fixture whose `node_modules` never materialized must FAIL naming the
/// link error, never pass with an empty program.
#[cfg(windows)]
fn create_junction_or_symlink(src: &Path, dest: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dest)
        .arg(src)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        junction => {
            let junction_note = match junction {
                Ok(s) => format!("mklink /J exited {s}"),
                Err(e) => format!("mklink /J failed to run: {e}"),
            };
            std::os::windows::fs::symlink_dir(src, dest)
                .map_err(|e| format!("{junction_note}; symlink_dir fallback failed: {e}"))
        }
    }
}

#[cfg(not(windows))]
fn create_junction_or_symlink(src: &Path, dest: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(src, dest).map_err(|e| format!("symlink failed: {e}"))
}

/// Materialize the fixture project. Every failure here is a hard FAILURE
/// naming its cause: a half-installed workspace must not be reported as a
/// clean type-check run.
fn setup_temp_project() -> (tempfile::TempDir, PathBuf) {
    let node_modules_src = workspace_root()
        .join("packages")
        .join("example")
        .join("node_modules");
    assert!(
        node_modules_src.join("vue").exists(),
        "fixture prerequisite missing: {} does not exist. The gated tsgo engine \
         resolved, so this workspace IS installed — run `pnpm install \
         --frozen-lockfile` to complete it. (A missing dependency here would \
         otherwise silently produce an unresolvable program and a vacuous pass.)",
        node_modules_src.join("vue").display()
    );

    let temp = tempfile::TempDir::new().expect("failed to create temp dir");
    let temp_path = temp.path().to_path_buf();
    copy_dir_recursive(&fixture_dir(), &temp_path).expect("failed to copy fixture");
    let nm_dest = temp_path.join("node_modules");
    if let Err(reason) = create_junction_or_symlink(&node_modules_src, &nm_dest) {
        panic!(
            "failed to link the fixture's node_modules ({} -> {}): {reason}",
            node_modules_src.display(),
            nm_dest.display()
        );
    }
    assert!(
        nm_dest.join("vue").exists(),
        "the fixture's node_modules link resolved to nothing: {} has no `vue`",
        nm_dest.display()
    );
    (temp, temp_path)
}

// ── The oracle ──────────────────────────────────────────────────────────

#[test]
fn js_sfcs_are_checked_as_javascript_and_ts_sfcs_as_typescript() {
    // Engine FIRST: a workspace with no tsgo candidate at all is also a
    // workspace with no `packages/example/node_modules`, and that combination
    // is the one legitimate skip. Every prerequisite checked after this point
    // is therefore a genuine half-installed tree and FAILS.
    let Some(engine) = harness::resolve_gated_engine(harness::gated_engine_resolution()) else {
        return;
    };
    let (temp_dir, project) = setup_temp_project();

    // ── Default tsconfig: `checkJs` absent (the vue-tsc baseline) ────────
    let default_run: GatedRun = harness::run_verter_tsc(
        harness::verter_tsc_command(&project, &engine, "tsconfig.json"),
        "tsconfig.json",
        RUN_DEADLINE,
    );
    let default_diags = default_run.diags();
    let default_stdout = default_run.stdout();

    // INSTRUMENT CHECK against the FAILING case. The run must have produced
    // real work: if the TypeScript SFC's genuine errors are missing, the engine
    // never checked this program and every "zero JS errors" assertion below
    // would be vacuously true.
    let ts_broken = for_file(default_diags, "/src/TsBroken.vue");
    assert!(
        ts_broken.iter().any(|d| d.ts_code() == 2322),
        "the TypeScript SFC's real errors must still be reported — otherwise this run \
         proves nothing: {:?}",
        codes(&ts_broken)
    );
    assert!(
        ts_broken.iter().any(|d| d.ts_code() == 2345),
        "every TypeScript error must survive, not just the first: {:?}",
        codes(&ts_broken)
    );

    // Negative: a SOURCE-MAPPED validation carrier must never leak its own
    // path. Production names them `{Component}_{hash}.jsx` / `.tsx` (they are
    // the only `.jsx`/`.tsx` files in the program — the fixture's own sources
    // are `.vue` and its dependencies ship `.d.ts`), and every one of them is
    // registered `RemapKind::SourceMapped`, so a diagnostic wearing that
    // extension means the remap silently stopped happening and the user is
    // being shown a path that does not exist on disk.
    //
    // The public-API stub is deliberately NOT covered: it is registered
    // `RemapKind::Passthrough` — its own position IS the reported position —
    // and its leaked name is pinned by the corpus parity oracle. Widening this
    // to "no generated path at all" would assert against a contract this branch
    // does not change. The stub always carries the `.vue.` infix before its
    // extension (`Name_<hash>.vue.ts` / `.vue.tsx` / `.vue.js` / `.vue.jsx`),
    // which is exactly what separates it from a validation carrier
    // (`Name_<hash>.tsx` / `.jsx`).
    for diag in default_diags {
        let file = diag.file().replace('\\', "/");
        let is_public_api_stub = [".vue.ts", ".vue.tsx", ".vue.js", ".vue.jsx"]
            .iter()
            .any(|suffix| file.ends_with(suffix));
        assert!(
            is_public_api_stub || (!file.ends_with(".jsx") && !file.ends_with(".tsx")),
            "a source-mapped validation carrier leaked its generated path instead of \
             remapping to the .vue source: {file}"
        );
    }

    // Positive: a JavaScript SFC contributes NOTHING under a tsconfig that
    // never asked for JavaScript checking — through EITHER generated root.
    //
    // `JsOptions.vue` is the second root: a no-setup Options-API SFC whose
    // authored `<script>` body is copied verbatim into the public-API stub, so
    // a `.vue.ts` stub name made `strict` report TS7006 on every untyped method
    // parameter even though the validation carrier was correctly `.jsx`.
    //
    // `JsExpose.vue` is the third: a JavaScript `<script setup>` whose
    // `defineExpose({ … })` makes the public-API stub copy the AUTHORED setup
    // body into a `declare`-bearing TypeScript surface — so `function
    // bump(step)` produced the same false TS7006 through a root the
    // Options-API relabelling never touched.
    //
    // `JsxOptions.vue` is the fourth, and it is a PARSE failure rather than a
    // check one: its authored body contains `<span class="badge">`, which a
    // `.js` ScriptKind cannot parse at all. Syntax diagnostics are reported for
    // JavaScript files whether or not `checkJs` is on, so collapsing `jsx` onto
    // `js` shows here even under the default tsconfig.
    for stem in ["JsSetup", "JsDoc", "JsOptions", "JsExpose", "JsxOptions"] {
        let js = mentioning(default_diags, stem);
        assert!(
            js.is_empty(),
            "a JavaScript SFC must not be typechecked as TypeScript under a default \
             tsconfig, through any generated root ({stem}): {:?}",
            codes(&js)
        );
    }

    // `JsExpose.vue` exposes `scrollTo(region = 'top', mode)` — a defaulted
    // parameter followed by a required one, which is ordinary JavaScript but
    // cannot be spelled `(region?: any, mode: any)` in a declaration: that is
    // TS1016, "a required parameter cannot follow an optional parameter". The
    // usage of all three exposed methods is covered from `TsParent.vue` (clean)
    // and `TsParentArity.vue` (arity enforced).
    //
    // Deliberately NOT asserted here: "no TS1016 for JsExpose". It was tried
    // and it does not DISCRIMINATE. Emitting the invalid `(region?: any, mode:
    // any)` shape was measured end to end against the gated engine — through
    // both the typecheck and the declaration-emit paths — and no TS1016 is
    // reported, because the member sits inside `ShallowUnwrapRef<{ … }>` and
    // the engine's grammar check never reaches it there; the invalid signature
    // is emitted into the `.d.ts` verbatim. (The same shape written directly in
    // a `.ts`/`.d.ts` file DOES report TS1016 on that engine, so the rule
    // itself is real.) An assertion that passes with and without the defect
    // would be worse than none — the discriminating guard is the rendered-shape
    // assertion in `verter_compiler`'s
    // `javascript_setup_body_never_enters_the_typescript_public_stub`, which
    // fails when every default is marked `?`.

    // Negative: a `lang="tsx"` Options-API SFC keeps its REAL error and gains
    // no syntax errors. Its authored `<span class="badge">` is JSX, which a
    // `.ts` ScriptKind parses as a type assertion — so collapsing `tsx` onto
    // `ts` replaces the type error below with a pile of TS1xxx parse errors.
    // Asserting only "no syntax errors" would pass on a file nothing checked;
    // asserting only the type error would pass while syntax errors piled up
    // beside it.
    let tsx_options = mentioning(default_diags, "TsxOptions");
    assert!(
        tsx_options.iter().any(|d| d.ts_code() == 2322),
        "a `lang=\"tsx\"` Options-API SFC must still report its real type error: {:?}",
        codes(&tsx_options)
    );
    for stem in ["TsxOptions", "JsxOptions"] {
        let syntax: Vec<_> = mentioning(default_diags, stem)
            .into_iter()
            .filter(|d| d.ts_code() < 2000)
            .collect();
        assert!(
            syntax.is_empty(),
            "an authored JSX body must reach a JSX-capable ScriptKind — these are \
             parse errors from a non-JSX one ({stem}): {:?}",
            codes(&syntax)
        );
    }

    // Positive: an SFC whose `<script>` and `<script setup>` disagree about
    // `lang` is REJECTED — reported in Verter's own error namespace and
    // generating NO companion at all. Vue's `compileScript` throws on exactly
    // this SFC, so there is no authored dialect to label a companion with, and
    // both available labels corrupt the result: TypeScript strict-checks the
    // JavaScript block, JavaScript deletes the TypeScript block's diagnostics.
    assert!(
        default_stdout.contains("VTER1002"),
        "a mixed-language SFC must be reported to the user, not silently \
         resolved: {default_stdout}"
    );
    assert!(
        default_stdout.contains("MixedLang.vue")
            && default_stdout
                .lines()
                .any(|line| line.contains("VTER1002") && line.contains("MixedLang.vue")),
        "the rejection must name the offending SFC: {default_stdout}"
    );
    // Negative: it is not reported for every SFC — every other fixture agrees
    // with itself, and a rule that fired on all of them would be no rule.
    assert_eq!(
        default_stdout
            .lines()
            .filter(|line| line.contains("VTER1002"))
            .count(),
        1,
        "exactly the one mixed-language SFC is rejected: {default_stdout}"
    );
    // And the POINT of the rejection: refusing means generating nothing, so the
    // JavaScript `<script setup>` beside the TypeScript `<script>` is never
    // emitted into a strict-checked root. `MixedLang.vue`'s setup block is
    // `function bump(step) { … }` — an untyped parameter that reports TS7006
    // the instant it lands in a `.ts`/`.tsx` companion, which is exactly the
    // false diagnostic this whole surface exists to remove. Reporting VTER1002
    // and THEN generating the bad carrier anyway would leave it in place.
    let mixed = mentioning(default_diags, "MixedLang");
    assert!(
        mixed.is_empty(),
        "a refused SFC contributes NO engine diagnostic through any generated \
         root — a TS7006 here means the carrier was generated after all: {:?}",
        codes(&mixed)
    );

    // Positive: a TypeScript PARENT can actually USE the JavaScript child's
    // exposed surface. This is the half that "no TS7006 in the child" does not
    // cover: keeping the authored JavaScript body out of the TypeScript stub
    // means `typeof bump` cannot resolve, and falling back to `bump: unknown`
    // would leave the child clean while every consumer got TS2571 ("Object is
    // of type 'unknown'") for calling a perfectly good method — the same false
    // diagnostic, relocated. `TsParent.vue` calls `child.bump(1)` through
    // `InstanceType<typeof JsExpose>`, so it is silent only if the member kept a
    // callable type.
    let parent = for_file(default_diags, "/src/TsParent.vue");
    assert!(
        parent.is_empty(),
        "a TypeScript parent must be able to call a JavaScript child's exposed \
         members — a `function` declaration named by a shorthand property AND a \
         method shorthand — since an `unknown` member moves the false diagnostic \
         to the consumer: {:?}",
        codes(&parent)
    );

    // And the other half of "callable": the exposed method's ARITY is still
    // checked. `TsParentArity.vue` calls a one-parameter method with two
    // arguments, which must be TS2554. A permissive `(...args: any[]) => any`
    // fallback would make the member callable and this call silently fine —
    // quieter than `unknown`, and wrong in a way no consumer could see.
    let arity = for_file(default_diags, "/src/TsParentArity.vue");
    assert_eq!(
        arity.iter().filter(|d| d.ts_code() == 2554).count(),
        2,
        "an exposed method must keep its authored arity, not degrade to a \
         variadic shape that accepts any call — both the too-many-arguments \
         call on `focus` and the too-few on `scrollTo` must be reported: {:?}",
        codes(&arity)
    );

    // Negative: TypeScript SFCs are untouched — a clean one stays clean, and an
    // Options-API one still reports its real error. Without the second half the
    // "JsOptions is silent" assertion above could just mean Options-API SFCs
    // are never checked at all.
    assert!(
        mentioning(default_diags, "TsClean").is_empty(),
        "a valid TypeScript SFC must stay clean: {:?}",
        codes(&mentioning(default_diags, "TsClean"))
    );
    assert!(
        for_file(default_diags, "/src/TsOptions.vue")
            .iter()
            .any(|d| d.ts_code() == 2322),
        "a TypeScript Options-API SFC must still report its real error: {:?}",
        codes(&for_file(default_diags, "/src/TsOptions.vue"))
    );

    // ── `checkJs: true`: JavaScript IS checked ──────────────────────────
    // Proves the fix is a LABEL, not a suppression: Verter did not "turn off"
    // JavaScript checking, it stopped claiming JavaScript files were
    // TypeScript. The user's own tsconfig still decides, through `extends`.
    let checkjs_run = harness::run_verter_tsc(
        harness::verter_tsc_command(&project, &engine, "tsconfig.checkjs.json"),
        "tsconfig.checkjs.json",
        RUN_DEADLINE,
    );
    let checkjs_diags = checkjs_run.diags();

    let js_setup = for_file(checkjs_diags, "/src/JsSetup.vue");
    assert!(
        js_setup.iter().any(|d| d.ts_code() == 7006),
        "under `checkJs: true` the JavaScript SFC's implicit-any errors must return, \
         remapped to the .vue source: {:?}",
        codes(&js_setup)
    );

    // The Options-API JavaScript SFC too — the root that F1 relabelled must
    // come back under `checkJs`, not stay permanently silent.
    let js_options = for_file(checkjs_diags, "/src/JsOptions.vue");
    assert!(
        js_options.iter().any(|d| d.ts_code() == 7006),
        "under `checkJs: true` the JavaScript Options-API SFC's implicit-any errors \
         must return: {:?}",
        codes(&js_options)
    );

    // And the `defineExpose` root, through the `.jsx` validation carrier.
    let js_expose = for_file(checkjs_diags, "/src/JsExpose.vue");
    assert!(
        js_expose.iter().any(|d| d.ts_code() == 7006),
        "under `checkJs: true` the exposing JavaScript SFC's implicit-any errors \
         must return: {:?}",
        codes(&js_expose)
    );

    // The JSX JavaScript SFC too: a JSX-capable ScriptKind is not a relaxed
    // one, so its untyped `bump(step)` still reports once JavaScript is checked.
    let jsx_options = for_file(checkjs_diags, "/src/JsxOptions.vue");
    assert!(
        jsx_options.iter().any(|d| d.ts_code() == 7006),
        "under `checkJs: true` the JSX JavaScript SFC's implicit-any errors must \
         return: {:?}",
        codes(&jsx_options)
    );

    // Authored JSDoc types are honoured, and the error remaps to the carrier.
    let jsdoc = for_file(checkjs_diags, "/src/JsDoc.vue");
    assert!(
        jsdoc.iter().any(|d| d.ts_code() == 2345),
        "a JSDoc-typed JavaScript error must surface under `checkJs: true`: {:?}",
        codes(&jsdoc)
    );

    // And TypeScript is unaffected by the JavaScript switch.
    assert!(
        for_file(checkjs_diags, "/src/TsBroken.vue")
            .iter()
            .any(|d| d.ts_code() == 2322),
        "TypeScript errors must be unaffected by `checkJs`: {:?}",
        codes(&for_file(checkjs_diags, "/src/TsBroken.vue"))
    );
    assert!(
        mentioning(checkjs_diags, "TsClean").is_empty(),
        "a valid TypeScript SFC must stay clean under `checkJs: true` too: {:?}",
        codes(&mentioning(checkjs_diags, "TsClean"))
    );

    // The refusal is not a `checkJs` artefact either: a refused SFC produces no
    // companion under ANY tsconfig, so it stays silent here too — while the
    // admitted JavaScript SFCs above all came back. Without this half, "no
    // TS7006 from MixedLang" could just mean JavaScript was never checked.
    let mixed_checkjs = mentioning(checkjs_diags, "MixedLang");
    assert!(
        mixed_checkjs.is_empty(),
        "a refused SFC generates nothing under `checkJs: true` either: {:?}",
        codes(&mixed_checkjs)
    );

    drop(temp_dir);
}

// ── The deadline's own precondition ─────────────────────────────────────

/// The active nextest profile — the one that will govern THIS run.
///
/// `NEXTEST_PROFILE` is deliberately preserved by the canonical gate (it
/// selects WHICH configuration runs, unlike the output-format variables the
/// gate strips), and CI sets it to `ci`. Checking `[profile.default]`
/// unconditionally would therefore validate a configuration that is not the one
/// in force.
fn active_nextest_profile() -> String {
    std::env::var("NEXTEST_PROFILE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

/// The body of `[profile.<name>]` — its own keys and NOTHING past its end.
///
/// Scanning on through later sections is how a missing key silently resolves to
/// a different profile's: with `[profile.default]`'s `slow-timeout` removed, a
/// forward search finds `[profile.ci]`'s and reports the wrong budget as
/// default's. The section ends at the next line that opens a table, which
/// includes `[[profile.default.overrides]]` — an override array is a different
/// table, not more of this profile's key set.
fn profile_section(config: &str, profile: &str) -> Option<String> {
    let header = format!("[profile.{profile}]");
    let start = config.lines().position(|line| line.trim() == header)? + 1;
    Some(
        config
            .lines()
            .skip(start)
            .take_while(|line| !line.trim_start().starts_with('['))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The `slow-timeout` nextest will apply under `profile`, or `None` when there
/// is none to apply.
///
/// nextest profiles inherit unset keys from `[profile.default]`, so the lookup
/// is the active profile's own value, else default's — and NOTHING else. When
/// neither declares it there is no hang protection at all, and this returns
/// `None` rather than falling through to whatever section happens to come next
/// in the file.
fn slow_timeout_for(config: &str, profile: &str) -> Option<String> {
    [profile, "default"]
        .iter()
        .filter_map(|name| profile_section(config, name))
        .find_map(|section| {
            section.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("slow-timeout = ")
                    .map(str::to_owned)
            })
        })
}

/// DISCRIMINATING: the runner's kill-deadline is strictly below the killer that
/// would otherwise reach it first, in the profile that will actually run.
///
/// A deadline above its parent's never applies: the parent kills the test
/// before the child bound can fire, and the wedged run is reported as a timeout
/// of the whole job rather than of this run, with no reason attached. The
/// canonical gate runs this suite under `cargo nextest`, whose selected profile
/// terminates a test at `slow-timeout.period x terminate-after`.
///
/// Fails against the pre-change harness, whose deadline was 300s against a 180s
/// nextest killer — and against a reader that scans past its section's end, or
/// that checks `default` while `NEXTEST_PROFILE` selects another profile.
#[test]
fn the_run_deadline_is_strictly_below_the_nextest_termination_deadline() {
    let config = std::fs::read_to_string(workspace_root().join(".config").join("nextest.toml"))
        .expect("the gate's nextest profile must be readable");

    let active = active_nextest_profile();
    let slow = slow_timeout_for(&config, &active).unwrap_or_else(|| {
        panic!(
            "neither [profile.{active}] nor [profile.default] declares `slow-timeout`, \
             so nextest applies NO termination deadline and the runner's own bound \
             has nothing to sit below"
        )
    });

    let field = |name: &str| -> u64 {
        let rest = slow
            .split_once(name)
            .unwrap_or_else(|| panic!("slow-timeout must declare {name}: {slow}"))
            .1;
        rest.trim_start_matches([' ', '=', '"'])
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be numeric: {slow}"))
    };
    let killer = std::time::Duration::from_secs(field("period") * field("terminate-after"));

    assert!(
        RUN_DEADLINE < killer,
        "the runner's deadline ({RUN_DEADLINE:?}) must fire BEFORE nextest's \
         termination deadline ({killer:?}) under profile `{active}`, or it never applies"
    );
    // The oracle makes TWO invocations back to back, so the budget it can spend
    // under its own deadlines must also fit.
    assert!(
        RUN_DEADLINE * 2 < killer,
        "both of the oracle's runs must fit inside nextest's termination deadline \
         under profile `{active}`: 2 x {RUN_DEADLINE:?} vs {killer:?}"
    );
}

/// The section reader stops at its own section's end and fails closed on a
/// missing key.
///
/// This is what stops the assertion above from validating the WRONG profile: a
/// forward scan that runs past `[profile.default]` picks up `[profile.ci]`'s
/// `slow-timeout` and reports it as default's, so removing or renaming
/// default's key looks like a pass.
#[test]
fn the_profile_reader_stops_at_its_own_section() {
    const CONFIG: &str = "\
[profile.default]
slow-timeout = { period = \"60s\", terminate-after = 3 }

[profile.ci]
slow-timeout = { period = \"9s\", terminate-after = 9 }

[[profile.default.overrides]]
slow-timeout = { period = \"120s\", terminate-after = 3 }
";

    let default = profile_section(CONFIG, "default").expect("[profile.default] exists");
    assert!(
        default.contains("60s"),
        "the section must carry its own keys: {default}"
    );
    assert!(
        !default.contains("9s") && !default.contains("120s"),
        "the section must stop at its own end — neither [profile.ci] nor an \
         overrides array is part of it: {default}"
    );

    let ci = profile_section(CONFIG, "ci").expect("[profile.ci] exists");
    assert!(
        ci.contains("9s") && !ci.contains("120s"),
        "each profile reads its own body: {ci}"
    );

    // A profile whose key was REMOVED yields a section with no `slow-timeout`,
    // not the next section's — which is what makes the lookup above fail closed
    // rather than silently validate a different configuration.
    const STRIPPED: &str = "\
[profile.default]

[profile.ci]
slow-timeout = { period = \"9s\", terminate-after = 9 }
";
    let stripped = profile_section(STRIPPED, "default").expect("[profile.default] exists");
    assert!(
        !stripped.contains("slow-timeout"),
        "a stripped section must not inherit the next one's key: {stripped}"
    );

    assert!(
        profile_section(CONFIG, "nope").is_none(),
        "an absent profile has no section"
    );

    // And the LOOKUP on top of it. `ci` overrides default's value; `default`
    // reads its own; a profile with no section of its own inherits default's,
    // exactly as nextest does. The last case is the fail-closed one: with
    // default's key removed there is NO deadline to sit below, and the lookup
    // must say so instead of reporting `[profile.ci]`'s 9s as if it were the
    // budget the run will actually get.
    assert!(
        slow_timeout_for(CONFIG, "ci").unwrap().contains("9s"),
        "the active profile's own value wins"
    );
    assert!(
        slow_timeout_for(CONFIG, "default").unwrap().contains("60s"),
        "default reads its own value"
    );
    assert!(
        slow_timeout_for(CONFIG, "absent").unwrap().contains("60s"),
        "a profile with no section inherits default's, as nextest does"
    );
    assert_eq!(
        slow_timeout_for(STRIPPED, "default"),
        None,
        "with no declaration to inherit, the lookup fails closed rather than \
         reporting another profile's budget"
    );
}

// ── The refusal, proved through emission and through a consumer ─────────

/// A refused SFC generates NO artifact, and importing it resolves through the
/// ambient shim instead of cascading a TS2307.
///
/// Separate from the oracle because it drives the EMITTING path. The oracle
/// runs `--noEmit` twice, which is the right shape for a diagnostic oracle and
/// the wrong shape for a "nothing was generated" claim: `--noEmit` suppresses
/// emission for every file, so a leaked public stub or declaration carrier for
/// the refused SFC would be invisible there. Here `--declaration
/// --declarationDir <dir> --listEmittedFiles` makes the artifact inventory
/// observable in two independent ways — what the CLI says it wrote, and what is
/// on disk.
///
/// Being its own `#[test]` also keeps the deadline budget honest: the oracle's
/// two runs and this one run each sit inside their own nextest process.
#[test]
fn a_refused_sfc_emits_no_artifact_and_its_importer_falls_back_to_the_ambient_shim() {
    let Some(engine) = harness::resolve_gated_engine(harness::gated_engine_resolution()) else {
        return;
    };
    let (temp_dir, project) = setup_temp_project();
    let out_dir = project.join("emitted-types");

    let run = harness::run_verter_tsc(
        harness::verter_tsc_emit_command(&project, &engine, "tsconfig.json", &out_dir),
        "declaration emit",
        RUN_DEADLINE,
    );
    let stdout = run.stdout();

    // The emitted-artifact inventory, from what the CLI reports.
    let listed: Vec<&str> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("TSFILE: "))
        .map(str::trim)
        .collect();

    // INSTRUMENT CHECK: emission really happened. Without this, "no MixedLang
    // artifact" would be vacuously true of a run that emitted nothing at all.
    assert!(
        listed.iter().any(|path| Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("TsClean"))),
        "an ADMITTED SFC must emit its declaration — otherwise this run proves \
         nothing about what a refused one does NOT emit: {listed:?}"
    );

    // The claim: the refused SFC contributes NO artifact, under any name.
    let leaked: Vec<&&str> = listed
        .iter()
        .filter(|path| {
            Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("MixedLang"))
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "a refused SFC must generate no companion of any kind — the emit path \
         wrote one: {leaked:?}"
    );

    // And independently, from the filesystem: the CLI's own report is not the
    // only witness, so a bug that emitted an artifact without listing it is
    // still caught.
    let mut on_disk: Vec<String> = Vec::new();
    let mut stack = vec![out_dir.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                on_disk.push(name.to_string());
            }
        }
    }
    assert!(
        on_disk.iter().any(|name| name.starts_with("TsClean")),
        "the declaration directory must really hold the admitted SFC's artifact \
         (the on-disk instrument check): {on_disk:?}"
    );
    assert!(
        !on_disk.iter().any(|name| name.starts_with("MixedLang")),
        "a refused SFC left an artifact on disk: {on_disk:?}"
    );

    // The refusal is still reported, in this run too — refusing quietly would
    // be its own defect.
    assert!(
        stdout.contains("VTER1002") && stdout.contains("MixedLang.vue"),
        "the refusal must be reported on the emitting path as well: {stdout}"
    );

    // The IMPORTER experience, pinned rather than assumed.
    // `ImportsRefused.vue` imports the refused SFC and renders it: with no
    // companion generated, the `./MixedLang.vue` specifier falls through to the
    // ambient `declare module '*.vue'` shim (`DefineComponent<{}, {}, any>`).
    // So it RESOLVES — no TS2307 — and the shim's permissive surface accepts
    // the usage. Refusing one file must not cascade an unresolved-module error
    // through every module that mentions it.
    let diags = run.diags();
    let refused_importer = mentioning(diags, "ImportsRefused");
    assert!(
        refused_importer.is_empty(),
        "importing a refused SFC must resolve through the ambient shim, not \
         cascade an unresolved-module error across the project: {:?}",
        codes(&refused_importer)
    );

    // The CONTRAST that stops the silence above from meaning "importers are not
    // checked at all": an import of an ADMITTED SFC gets the real generated
    // surface, and a wrong call against it IS caught. `TsParentArity.vue` calls
    // `JsExpose`'s one-parameter exposed method with two arguments.
    let admitted_importer = for_file(diags, "/src/TsParentArity.vue");
    assert!(
        admitted_importer.iter().any(|d| d.ts_code() == 2554),
        "an import of an ADMITTED SFC must carry its real surface, so these \
         importer modules are demonstrably being checked: {:?}",
        codes(&admitted_importer)
    );

    drop(temp_dir);
}
