//! The §4 GENERATION SPIKE — empirically validates, against the PINNED tsgo
//! `7.0.0-dev.20260526.1`, the BLOCKING assumptions the harness's generation
//! side rests on (`docs/arch/u0-oracle-harness-design.md` §4 "Spike").
//!
//! FEATURE-GATED (`oracle-gen`) and `#[cfg(test)]`: it drives tsgo via
//! `verter_type_runtime`'s `TsgoTypeProvider` (Q3 — the adopted LSP driver) and
//! is NEVER part of the DEFAULT gate (the default `cargo nextest run --workspace`
//! / `cargo test -p verter_session` runs do not enable `oracle-gen`, so tsgo
//! stays out of that closure — pinned by
//! `oracle_tsgo_forbidden::tsgo_not_reachable_from_resolver`). It lives OUTSIDE
//! the `oracle/` consumption subtree, so the consumption-path scan
//! (`oracle_consumption_path_has_no_tsgo_spawn`) stays tsgo-free.
//!
//! Run with: `cargo test -p verter_session --features oracle-gen oracle_gen_spike`.
//!
//! The spike is a GATING design input: it proves a construct/mode/primitive
//! class is admissible. It reuses the PURE harness core (`probe`,
//! `hover_extract`, `admission`, `normalize`) so the empirical proof runs the
//! SAME pipeline the generator will — not a parallel re-derivation.
//!
//! Empirically PROVEN here (the foundational items): the probe-driven hover
//! EXPANDS the alias (Q2 probe form), the hover-extraction grammar recovers the
//! RHS, the admitted RHS lowers + normalizes CONFLUENTLY with the authored
//! spelling (the central soundness obligation), the binding-identity primitive
//! (`textDocument/definition`) BINDS to the intended declaration (the anti-shadow
//! primitive the design flagged as unverified — `anti_shadow_needs_proven_binding_primitive`),
//! and a clean probe yields ZERO diagnostics (`probe_binds_to_registry_target`).
//!
//! Remaining BLOCKING spike items (documented, not yet automated here): the
//! multi-option `compilerOptions` delivery matrix, the vendored-lib forcing
//! mechanism, and confluence over the FULL hard-family corpus.

use std::time::Duration;

use verter_type_runtime::tsgo::ipc::{find_tsgo_binary, TsgoTypeProvider};
use verter_type_runtime::TypeProvider;

use super::oracle::admission::AdmissionVerdict;
use super::oracle::normalize::ProjectionModeKind;
use super::oracle::{admission, hover_extract, normalize, probe};

/// The canonical fixture from the design's worked example (Q1 §"Concrete
/// example"): a multi-member object alias.
const CANONICAL_FIXTURE: &str =
    "type ComposedProps = { id: number; label: string; tag?: \"a\" | \"b\" };\n";

/// A minimal standalone-host oracle tsconfig (the §Q2 canonical config shape; the
/// full closed effective-option pin + vendored corpus is a separate increment).
const ORACLE_TSCONFIG: &str =
    "{ \"compilerOptions\": { \"strict\": true, \"target\": \"es2020\", \"moduleResolution\": \"bundler\" } }\n";

/// Spawn tsgo over a temp workspace, open `source` as `<root>/fixture.ts`, and
/// return the spawned provider + the absolute fixture path. Returns `None` if
/// tsgo is not installed (the spike SKIPS rather than failing in such an env).
async fn spawn_over(source: &str) -> Option<(TsgoTypeProvider, String, tempfile::TempDir)> {
    let tsgo_bin = match find_tsgo_binary() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("oracle_gen_spike: SKIP — tsgo binary not found: {e}");
            return None;
        }
    };
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("tsconfig.json"), ORACLE_TSCONFIG).expect("write tsconfig");
    let fixture_path = dir.path().join("fixture.ts");
    std::fs::write(&fixture_path, source).expect("write fixture");
    let root_uri = format!("file://{}", dir.path().display());

    let provider = match tokio::time::timeout(
        Duration::from_secs(30),
        TsgoTypeProvider::spawn(&tsgo_bin, &root_uri),
    )
    .await
    {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            eprintln!("oracle_gen_spike: SKIP — tsgo spawn failed: {e}");
            return None;
        }
        Err(_) => {
            eprintln!("oracle_gen_spike: SKIP — tsgo spawn timed out");
            return None;
        }
    };
    let path = fixture_path.to_string_lossy().into_owned();
    let _ = provider.open_file(&path, source).await;
    Some((provider, path, dir))
}

/// PROOF 1 — the probe-driven hover EXPANDS the alias to its structural body,
/// the extraction grammar recovers the RHS, the admission gate ADMITS it, and the
/// lowered+normalized hover value is CONFLUENT with the authored spelling.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spike_hover_expands_and_is_confluent_with_authored() {
    let ordinal = 0u16;
    let rhs = probe::resolve_expr_probe_rhs("ComposedProps", &[]).expect("empty type_args RHS");
    let synth = probe::append_probe(CANONICAL_FIXTURE, ordinal, &rhs);

    let Some((provider, path, _dir)) = spawn_over(&synth.source).await else {
        return; // tsgo absent — skip
    };

    let hover = tokio::time::timeout(
        Duration::from_secs(15),
        provider.get_hover(&path, synth.probe_name_offset as u32),
    )
    .await
    .expect("hover did not time out")
    .expect("hover request ok")
    .expect("hover present at probe");

    // The hover EXPANDED the alias: it names the probe and prints the members
    // (not just the alias name `ComposedProps`).
    assert!(
        hover.contents.contains(&probe::probe_name(ordinal)),
        "hover must name the probe; got: {}",
        hover.contents
    );
    assert!(
        hover.contents.contains("id")
            && hover.contents.contains("label")
            && hover.contents.contains("tag"),
        "hover must print the expanded members; got: {}",
        hover.contents
    );

    // The extraction grammar recovers the RHS from the markdown hover.
    let extracted = hover_extract::extract_probe_rhs(&hover.contents, &probe::probe_name(ordinal))
        .expect("extract probe RHS from hover");

    // The admission gate ADMITS the expanded hover RHS (no lossy construct).
    assert!(
        matches!(
            admission::admit_hover_text(&extracted),
            AdmissionVerdict::Admit
        ),
        "expanded object hover must be admitted; RHS = {extracted}"
    );

    // CONFLUENCE: the hover spelling and the AUTHORED spelling both lower +
    // normalize to the SAME canonical form (the central soundness obligation,
    // over tsgo's ACTUAL hover spelling — not an assumed one).
    let hover_expr = admission::lower_hover_rhs(&extracted).expect("hover RHS lowers cleanly");
    let authored_expr =
        admission::lower_hover_rhs("{ id: number; label: string; tag?: \"a\" | \"b\" }")
            .expect("authored RHS lowers cleanly");
    let hover_norm =
        normalize::normalized_canonical_json(&hover_expr, ProjectionModeKind::Expanded)
            .expect("hover value normalizes");
    let authored_norm =
        normalize::normalized_canonical_json(&authored_expr, ProjectionModeKind::Expanded)
            .expect("authored value normalizes");
    assert_eq!(
        hover_norm, authored_norm,
        "tsgo hover spelling and authored spelling must normalize byte-equal (confluence)"
    );
}

/// PROOF 2 — the binding-identity primitive (`textDocument/definition`) is
/// PROVEN at the pinned tsgo: the probe's RHS reference binds to the intended
/// `ComposedProps` declaration, not a shadow/ambient. The design flagged this
/// primitive as UNVERIFIED (`anti_shadow_needs_proven_binding_primitive`); this
/// proves it works, so anti-shadow-needing rows become admissible.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spike_definition_primitive_binds_to_intended_decl() {
    let ordinal = 0u16;
    let synth = probe::append_probe(CANONICAL_FIXTURE, ordinal, "ComposedProps");
    let Some((provider, path, _dir)) = spawn_over(&synth.source).await else {
        return;
    };
    // Offset of the `ComposedProps` reference in the probe RHS (after ` = `).
    let rhs_ref_offset = synth
        .source
        .find("= ComposedProps;")
        .expect("find probe rhs")
        + 2;
    let defs = tokio::time::timeout(
        Duration::from_secs(15),
        provider.get_definition(&path, rhs_ref_offset as u32),
    )
    .await
    .expect("definition did not time out")
    .expect("definition request ok");

    assert!(
        !defs.is_empty(),
        "textDocument/definition must return the decl location (the anti-shadow primitive)"
    );
    // The definition lands on the `ComposedProps` declaration site (offset 5 —
    // the `type ` prefix is 5 bytes — through its name end), in the same file.
    let decl_off = CANONICAL_FIXTURE.find("ComposedProps").expect("decl") as u32;
    assert!(
        defs.iter().any(|d| d.path.ends_with("fixture.ts")
            && d.start <= decl_off + 1
            && d.end >= decl_off),
        "definition must bind to the ComposedProps declaration; got {defs:?}"
    );
}

/// PROOF 3 — a clean probe yields ZERO diagnostics (the zero-new-diagnostics half
/// of `probe_binds_to_registry_target`). A non-clean probe would surface a
/// diagnostic the generator's gate catches.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spike_clean_probe_has_zero_diagnostics() {
    let synth = probe::append_probe(CANONICAL_FIXTURE, 0, "ComposedProps");
    let Some((provider, path, _dir)) = spawn_over(&synth.source).await else {
        return;
    };
    let diags = tokio::time::timeout(Duration::from_secs(15), provider.get_diagnostics(&path))
        .await
        .expect("diagnostics did not time out")
        .expect("diagnostics request ok");
    assert!(
        diags.is_empty(),
        "a clean probe must produce zero diagnostics; got {diags:?}"
    );
}
