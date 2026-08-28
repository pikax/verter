//! Surface 3 replacement, half two of two (half one is
//! `cargo check --workspace --all-targets --profile no-debug-assertions`,
//! wired into `scripts/gate.mjs`).
//!
//! This crate carries no production code. It exists to be compiled and its
//! tests RUN exactly once, scoped to this package, under
//! `--cargo-profile no-debug-assertions` — never as part of a whole-workspace
//! archive and never under the default `dev` profile. See the workspace
//! `Cargo.toml`'s `[profile.no-debug-assertions]` doc comment and
//! `docs/contributing/gate-performance.md`.
//!
//! What belongs here: behaviour that can differ ONLY because
//! `debug_assertions` / `overflow-checks` are off, or because a
//! `#[cfg(debug_assertions)]` block is absent. It is deliberately small
//! ("dozens of tests at most") — it is not a second copy of the
//! `verter_session` suite. The two profile-sanity tests
//! ([`profile_contract::debug_assertions_are_off_under_this_profile`] and
//! [`profile_contract::overflow_checks_are_off_under_this_profile`]) exist so
//! a misconfigured invocation (e.g. someone runs this crate under `dev`
//! instead of the alternate profile) fails LOUD instead of silently passing
//! zero-signal tests — see the deletion-bar row "shipped configuration
//! silently selects zero tests" in the maintainer directive above.
//!
//! Coverage scope, by crate (audited by grep over each crate's `src/` for
//! `cfg(debug_assertions)` — the conditional-compilation mechanism this
//! guard's compile-only half already fully covers regardless of behavioral
//! test count):
//! - `verter_session`: FOUR production `#[cfg(debug_assertions)]` blocks
//!   (`parse.rs`, `resolver_core/runtime_values.rs`,
//!   `meta_resolve/projection_demand.rs`, `host_manage/eval_env.rs`) — all
//!   non-breaking oracle cross-checks. `shipped_cfg_behaviour` below pins
//!   the observable result of the surviving fast path through real
//!   `VerterHost` scenarios.
//! - `verter_compiler`: ZERO. `compiler_behaviour` below is a forward-
//!   looking behavioral pin on real template-codegen arithmetic
//!   (`StandaloneCompiler`, bypassing `verter_session`), not a response to
//!   a known hazard.
//! - `verter_scheduler`: ZERO.
//!   `shipped_cfg_behaviour::compile_many_batch_dispatch_stays_correct_under_shipped_cfg`
//!   is the same kind of forward-looking pin, scoped to the batch/pool
//!   dispatch path specifically (the single-upsert tests exercise the
//!   scheduler only incidentally).
//!
//! `bf2-authoritative` (a `verter_session` feature gating that crate's OWN
//! `#[cfg(test)]` oracle-comparison suite) is NOT reachable from here at
//! any feature setting: a `[dev-dependencies]` edge links a dependency's
//! LIBRARY target only, never its `#[cfg(test)]` code — that suite runs
//! only when `verter_session` is the crate under test.

#[cfg(test)]
mod profile_contract {
    /// Canary: this crate is worthless unless it is actually compiled under
    /// `no-debug-assertions`. If `debug_assertions` is on here, every other
    /// test in this crate is running under the SAME profile the normal
    /// nextest universe already covers, proving nothing new. Fails loudly
    /// rather than silently degrading to a no-op run.
    #[test]
    // The whole point of this assertion is that `cfg!(debug_assertions)` is a
    // compile-time constant that differs by profile — `dev` makes it
    // constant-true (this fails loud, by design) and `no-debug-assertions`
    // makes it constant-false (this passes). clippy's "always resolves the
    // same way in THIS compilation" observation is correct and not a bug to
    // fix; `assertions_on_constants`'s `const { assert!(..) }` suggestion
    // would just move the same profile-conditional panic to compile time.
    #[allow(clippy::assertions_on_constants)]
    fn debug_assertions_are_off_under_this_profile() {
        assert!(
            !cfg!(debug_assertions),
            "verter_shipped_cfg_contract must be invoked with \
             `--cargo-profile no-debug-assertions` (see scripts/gate.mjs); \
             it observed debug_assertions ON, which means it is running \
             under the ordinary dev profile and adds no coverage beyond \
             surface 1."
        );
    }

    /// Canary: `overflow-checks` must also be off under this profile (the
    /// workspace `[profile.no-debug-assertions]` sets both). Proven by
    /// executing an operation that WOULD panic with overflow checks on and
    /// asserting it silently wraps instead — the only way this assertion can
    /// pass is if overflow checks are genuinely off in the compiled binary.
    #[test]
    fn overflow_checks_are_off_under_this_profile() {
        let near_max: u8 = 250;
        let bump: u8 = std::hint::black_box(10);
        let result: Result<u8, _> = std::panic::catch_unwind(|| near_max + bump);
        assert!(
            result.is_ok(),
            "250u8 + 10u8 must not panic under overflow-checks=false; a panic \
             means this crate is not running under the no-debug-assertions profile."
        );
        assert_eq!(
            result.unwrap(),
            4u8,
            "250u8 + 10u8 must silently wrap to 4 under overflow-checks=false."
        );
    }
}

/// Shared VerterHost fixture helpers for the behavioural contract tests
/// below. Kept in a `#[cfg(test)]` module (not a dev-dependency-only
/// `tests/` binary) — this crate's entire purpose is to run under the
/// alternate profile, so its "integration" tests are ordinary unit tests
/// against the `verter_session` dev-dependency.
#[cfg(test)]
mod support {
    use std::sync::Arc;
    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

    pub fn build_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig::default())
    }

    pub fn upsert(host: &VerterHost, id: &str, src: &str, lang: FileLanguage) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: id.to_string(),
                source: Arc::from(src),
                file_language: lang,
                aliases: Vec::new(),
            })
            .unwrap_or_else(|e| panic!("upsert {id}: {e:?}"));
    }
}

/// Behavioural coverage: real `VerterHost` scenarios that pass through the
/// production sites this block's audit found relying on
/// `#[cfg(debug_assertions)]` non-breaking readiness cross-checks
/// (`host_manage/eval_env.rs`, `parse.rs`, `resolver_core/runtime_values.rs`)
/// — those blocks compare a graph-native fast path against an oracle and
/// `debug_assert_eq!`/`assert_eq!` on divergence, then return the SAME value
/// either way. Under `no-debug-assertions` the cross-check itself compiles
/// out; these tests instead pin the OBSERVABLE result those code paths
/// produce, so a shipped-cfg-only regression in the surviving fast path
/// still fails here even though the debug-only oracle comparison cannot run.
#[cfg(test)]
mod shipped_cfg_behaviour {
    use super::support::{build_host, upsert};
    use verter_session::FileLanguage;

    #[test]
    fn vue_cross_file_define_props_resolves_under_shipped_cfg() {
        let host = build_host();
        upsert(
            &host,
            "/src/props.ts",
            "export interface Foo { a: number; b: string; }\n",
            FileLanguage::script_ts(),
        );
        upsert(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Foo } from './props';\n\
             defineProps<Foo>();\n\
             </script>\n\
             <template><div /></template>\n",
            FileLanguage::vue(),
        );
        let meta = host
            .get_component_meta("/src/Comp.vue")
            .expect("defineProps<Foo>() must resolve under the shipped-cfg profile");
        let mut names: Vec<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["a", "b"],
            "cross-file prop type resolution must produce both Foo members under shipped cfg"
        );
    }

    #[test]
    fn svelte_props_rune_resolves_under_shipped_cfg() {
        let host = build_host();
        upsert(
            &host,
            "/src/Comp.svelte",
            "<script lang=\"ts\">\n\
             let { title, count }: { title: string; count: number } = $props();\n\
             </script>\n\
             <div>{title}: {count}</div>\n",
            FileLanguage::svelte(),
        );
        let meta = host
            .get_component_meta("/src/Comp.svelte")
            .expect("$props() destructure must resolve under the shipped-cfg profile");
        let mut names: Vec<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["count", "title"]);
    }

    #[test]
    fn declaration_merge_unions_members_under_shipped_cfg() {
        // Two same-name `interface Foo` declarations must merge into the
        // union of their members (Declaration Merging (CRITICAL) in
        // CLAUDE.md) — exercises the `EvalEnv` ordered-contributor-group
        // path under shipped cfg, not just under the dev profile the
        // architecture guards already cover.
        let host = build_host();
        upsert(
            &host,
            "/src/props.ts",
            "export interface Foo { a: number; }\n\
             export interface Foo { b: string; }\n",
            FileLanguage::script_ts(),
        );
        upsert(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Foo } from './props';\n\
             defineProps<Foo>();\n\
             </script>\n\
             <template><div /></template>\n",
            FileLanguage::vue(),
        );
        let meta = host
            .get_component_meta("/src/Comp.vue")
            .expect("merged Foo must resolve under the shipped-cfg profile");
        let mut names: Vec<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["a", "b"],
            "same-name interface merge must union both contributors' members under shipped cfg"
        );
    }

    #[test]
    fn pick_utility_selects_only_named_member_under_shipped_cfg() {
        let host = build_host();
        upsert(
            &host,
            "/src/props.ts",
            "export interface Foo { a: number; b: string; c: boolean; }\n",
            FileLanguage::script_ts(),
        );
        upsert(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Foo } from './props';\n\
             defineProps<Pick<Foo, 'a'>>();\n\
             </script>\n\
             <template><div /></template>\n",
            FileLanguage::vue(),
        );
        let meta = host
            .get_component_meta("/src/Comp.vue")
            .expect("Pick<Foo, 'a'> must resolve under the shipped-cfg profile");
        let names: Vec<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["a"],
            "Pick<Foo, 'a'> must publish exactly the picked member under shipped cfg"
        );
    }

    #[test]
    fn repeated_upsert_of_same_file_stays_consistent_under_shipped_cfg() {
        let host = build_host();
        upsert(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             defineProps<{ a: number }>();\n\
             </script>\n\
             <template><div /></template>\n",
            FileLanguage::vue(),
        );
        let first = host
            .get_component_meta("/src/Comp.vue")
            .expect("first resolve");
        assert_eq!(
            first
                .props
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a"]
        );

        upsert(
            &host,
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             defineProps<{ a: number; b: string }>();\n\
             </script>\n\
             <template><div /></template>\n",
            FileLanguage::vue(),
        );
        let second = host
            .get_component_meta("/src/Comp.vue")
            .expect("second resolve after edit");
        let mut names: Vec<_> = second.props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["a", "b"],
            "an in-place edit must invalidate and re-resolve under shipped cfg, not serve a stale result"
        );
    }

    #[test]
    fn scheduler_shutdown_and_restart_replays_cleanly_under_shipped_cfg() {
        // Create -> use -> drop (runs `impl Drop for Scheduler`, which sets
        // the shutdown flag and joins the driver thread — scheduler.rs) ->
        // create again in the SAME process. Exercises the DAG-waiter /
        // stranding invariants `scheduler.rs` guards with `debug_assert!`
        // under a profile where those guards compile out.
        for i in 0..3 {
            let host = build_host();
            let path = format!("/src/Comp{i}.vue");
            upsert(
                &host,
                &path,
                "<script setup lang=\"ts\">\ndefineProps<{ n: number }>();\n</script>\n\
                 <template><div /></template>\n",
                FileLanguage::vue(),
            );
            let meta = host.get_component_meta(&path).unwrap_or_else(|| {
                panic!("iteration {i} must resolve after a fresh scheduler start")
            });
            assert_eq!(meta.props.len(), 1, "iteration {i} prop count");
            drop(host);
        }
    }

    /// A batch scheduler-dispatch path (`compile_many`), specifically
    /// exercising `verter_scheduler`'s pool/coordination logic under the
    /// shipped-cfg profile — the single-upsert tests above exercise the
    /// scheduler only incidentally (one file, one implicit submission).
    /// `verter_scheduler` has NO `#[cfg(debug_assertions)]` conditional-
    /// compilation blocks today (confirmed by grep over `crates/
    /// verter_scheduler/src`, so the compile-only half of the shipped-cfg
    /// guard already covers the "item hidden behind cfg(debug_assertions)"
    /// hazard completely for this crate) — this test's value is behavioral:
    /// pinning that concurrent batch dispatch through the pool still
    /// produces the correct, per-input-deterministic result set under the
    /// shipped configuration, as a regression net for future scheduler
    /// changes and for the overflow-checks-off class in general.
    #[test]
    fn compile_many_batch_dispatch_stays_correct_under_shipped_cfg() {
        use verter_session::host_compile::{CompileBatchInput, CompileBatchOptions};

        let host = build_host();
        let inputs: Vec<CompileBatchInput> = (0..12)
            .map(|i| CompileBatchInput {
                canonical_id: format!("/src/Batch{i}.vue"),
                source: std::sync::Arc::from(format!(
                    "<script setup lang=\"ts\">const n = {i}</script>\
                     <template><div>{{{{ n }}}}</div></template>"
                )),
                requested_mode: None,
                component_id: None,
            })
            .collect();

        let results = host.compile_many(
            inputs,
            CompileBatchOptions::default(),
            verter_session::host_compile::CompileManyTarget::HostBacked,
        );
        assert_eq!(results.len(), 12, "one entry per original input position");

        for (i, entry) in results.iter().enumerate() {
            assert_eq!(
                entry.canonical_id,
                format!("/src/Batch{i}.vue"),
                "batch position {i} must report its OWN canonical id under shipped cfg"
            );
            match &entry.outcome {
                verter_session::host_compile::CompileBatchOutcome::Produced { code, .. } => {
                    assert!(
                        code.contains(&format!("const n = {i}")),
                        "batch position {i}'s compiled code must reflect its OWN \
                         content (`const n = {i}`), not a sibling's, under shipped \
                         cfg: {code}"
                    );
                }
                verter_session::host_compile::CompileBatchOutcome::Failed { errors } => {
                    panic!("batch position {i} must compile under shipped cfg: {errors:?}")
                }
            }
        }
    }
}

/// Direct (non-`verter_session`-mediated) coverage of `verter_compiler`'s
/// own codegen under the shipped-cfg profile, through the crate's public
/// [`verter_compiler::standalone::StandaloneCompiler`] boundary.
/// `verter_compiler` has NO `#[cfg(debug_assertions)]` conditional-
/// compilation blocks today (confirmed by grep over `crates/verter_compiler
/// /src`, so the compile-only half of the shipped-cfg guard already covers
/// the "item hidden behind cfg(debug_assertions)" hazard completely for
/// this crate) — the tests below are behavioral, pinning the template
/// codegen path's OBSERVABLE output (which walks the source-bounds-checked
/// helpers in `template/code_gen/shared/helpers.rs`, among others) so a
/// shipped-cfg-only regression in that arithmetic still fails here.
#[cfg(test)]
mod compiler_behaviour {
    use verter_compiler::compile::{VueExecutionInputs, VueMacroSemanticInput};
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, ProductKind,
        RuntimeProductRequest, VueCompileRequest,
    };
    use verter_compiler::standalone::{DirectExecutionInputs, StandaloneCompiler};

    #[test]
    fn v_for_and_interpolation_codegen_is_well_formed_under_shipped_cfg() {
        // A template shaped to walk the bounds-checked span/offset helpers
        // in `template/code_gen/shared/helpers.rs` (element tag names,
        // interpolation expression spans, and a `v-for` iteration) — real
        // production template-codegen arithmetic, not a synthetic probe.
        let source = "<template>\n\
             <ul>\n\
             <li v-for=\"item in items\" :key=\"item.id\">{{ item.label }}</li>\n\
             </ul>\n\
             </template>\n";

        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("a single RuntimeClient product must construct");

        let execution_inputs = VueExecutionInputs::default();
        let output = StandaloneCompiler
            .compile(
                source,
                &request,
                DirectExecutionInputs::Vue {
                    execution: &execution_inputs,
                    macros: &VueMacroSemanticInput::Unavailable,
                },
            )
            .expect("a v-for + interpolation template must not be refused under shipped cfg");

        assert!(
            output.diagnostics.is_empty(),
            "template codegen must not error under shipped cfg: {:?}",
            output.diagnostics
        );
        let code = output
            .artifacts
            .artifact(ProductKind::RuntimeClient)
            .expect("RuntimeClient must produce an artifact")
            .code();
        assert!(
            code.contains("renderList"),
            "v-for codegen must emit the renderList helper call under shipped cfg: {code}"
        );
        assert!(
            code.contains("item.label"),
            "the interpolation expression must survive codegen under shipped cfg: {code}"
        );
    }
}
