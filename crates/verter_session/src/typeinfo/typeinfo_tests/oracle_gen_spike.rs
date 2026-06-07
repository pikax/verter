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
//! Also PROVEN here: the multi-option `compilerOptions` delivery matrix
//! (`oracle_options_delivery_proven` — tsgo reads the root tsconfig and applies
//! `exactOptionalPropertyTypes` / `strictNullChecks` / `noUncheckedIndexedAccess`)
//! and the vendored-lib forcing mechanism (the NAMED §Q2 candidate: `"noLib":
//! true` plus an explicit corpus-rooted lib list forces tsgo off its
//! native-bundled libs while honouring the vendored ones).
//!
//! Remaining BLOCKING spike items (documented, not yet automated here): confluence
//! over the FULL hard-family corpus (mapped / conditional / template / tuple /
//! class families each need their per-class verdict beyond the object-alias core
//! proven above).

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
///
/// `exactOptionalPropertyTypes: true` is LOAD-BEARING for the confluence proof
/// (§4 item 1a, the central soundness obligation): Verter's `TypeExpr` models an
/// optional member as `{ optional: true, ty: T }` with NO `| undefined` arm, so
/// for the hover-lowered side to converge with Verter's projection, tsgo must
/// print an optional member as `tag?: T` (NOT `tag?: T | undefined`). Under
/// tsgo's default (`exactOptionalPropertyTypes: false`, NOT enabled by `strict`)
/// an optional property's APPARENT type includes `| undefined` in hover, which
/// diverges from Verter's representation. The option is a print-affecting member
/// of the §Q2 closed effective-option table — pinned `true` here so the two sides
/// are confluent for optional members. This is the `exactOptionalPropertyTypes`
/// row of the `oracle_options_delivery_proven` matrix proven below.
const ORACLE_TSCONFIG: &str = "{ \"compilerOptions\": { \"strict\": true, \"exactOptionalPropertyTypes\": true, \"target\": \"es2020\", \"moduleResolution\": \"bundler\" } }\n";

/// Spawn tsgo over a temp workspace with the given `tsconfig`, write + open every
/// `(relative_name, content)` in `files`, and return the spawned provider plus the
/// absolute path of the FIRST listed file (the primary fixture). Returns `None` if
/// tsgo is not installed (the spike SKIPS rather than failing in such an env).
///
/// This is the one driver shape the generator (H) will reuse: a frozen root with a
/// vendored `tsconfig.json` (the option-delivery channel — tsgo reads the config
/// from the root) plus the program files written into that root.
async fn spawn_with(
    tsconfig: &str,
    files: &[(&str, &str)],
) -> Option<(TsgoTypeProvider, String, tempfile::TempDir)> {
    let tsgo_bin = match find_tsgo_binary() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("oracle_gen_spike: SKIP — tsgo binary not found: {e}");
            return None;
        }
    };
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("tsconfig.json"), tsconfig).expect("write tsconfig");
    let mut primary: Option<String> = None;
    for (name, content) in files {
        let p = dir.path().join(name);
        std::fs::write(&p, content).unwrap_or_else(|e| panic!("write {name}: {e}"));
        if primary.is_none() {
            primary = Some(p.to_string_lossy().into_owned());
        }
    }
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
    for (name, content) in files {
        let p = dir.path().join(name).to_string_lossy().into_owned();
        let _ = provider.open_file(&p, content).await;
    }
    let path = primary.expect("at least one file");
    Some((provider, path, dir))
}

/// Spawn tsgo over a temp workspace under the canonical `ORACLE_TSCONFIG`, opening
/// `source` as `<root>/fixture.ts`.
async fn spawn_over(source: &str) -> Option<(TsgoTypeProvider, String, tempfile::TempDir)> {
    spawn_with(ORACLE_TSCONFIG, &[("fixture.ts", source)]).await
}

/// Hover the alias-name probe for `source` (appended `type __oracle_probe__0 = <rhs>;`)
/// under `tsconfig`, returning the raw hover contents — the option-delivery matrix's
/// observation primitive. `None` ⟹ tsgo absent (skip).
async fn hover_probe_under(tsconfig: &str, source: &str, rhs: &str) -> Option<String> {
    let synth = probe::append_probe(source, 0, rhs);
    let (provider, path, _dir) = spawn_with(tsconfig, &[("fixture.ts", &synth.source)]).await?;
    let hover = tokio::time::timeout(
        Duration::from_secs(15),
        provider.get_hover(&path, synth.probe_name_offset as u32),
    )
    .await
    .expect("hover did not time out")
    .expect("hover request ok")
    .expect("hover present at probe");
    Some(hover.contents)
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

// ---------------------------------------------------------------------------
// BLOCKING §4 spike items: option-delivery matrix + vendored-lib forcing.
// ---------------------------------------------------------------------------

/// One row of the `oracle_options_delivery_proven` matrix: a print-affecting
/// option, a discriminating fixture, the tsconfig that pins the option to each of
/// its two values, and the substring each hover MUST / MUST NOT contain. A
/// differing hover across the two configs proves tsgo READ + APPLIED that option
/// from the delivered (root) tsconfig.
struct OptionDeliveryCase {
    option: &'static str,
    fixture: &'static str,
    probe_rhs: &'static str,
    /// tsconfig that yields the `with` hover (the option set to the value whose
    /// effect we assert) and the tsconfig that yields the `without` hover.
    cfg_with: &'static str,
    cfg_without: &'static str,
    /// Substring the `with` hover MUST contain and the `without` hover MUST NOT —
    /// the discriminator the option toggles.
    discriminator: &'static str,
}

/// PROOF 4 — the multi-option `compilerOptions` delivery-proof MATRIX
/// (`oracle_options_delivery_proven`). For each print-affecting option the §Q2
/// closed effective-option table pins, drive a DISCRIMINATING fixture under the
/// config that sets the option vs the config that clears it, and assert the
/// `discriminator` substring is PRESENT in one hover and ABSENT in the other AND
/// the two hovers DIFFER. A differing hover proves tsgo READ + APPLIED that option
/// from the delivered (root) tsconfig — so the delivery channel carries EVERY
/// pinned option, not just one (a single-flag probe would miss a dropped second
/// flag; the matrix does not). Covers two of the three named print-affecting
/// options via the TYPE-alias probe: `exactOptionalPropertyTypes` and
/// `strictNullChecks`. The third, `noUncheckedIndexedAccess`, is an element-access
/// EXPRESSION-level effect (it does not surface on an indexed-access TYPE at the
/// pinned tsgo), so it is proven separately via a VALUE probe in
/// `spike_nuia_delivery_via_value_probe`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spike_options_delivery_matrix() {
    // exactOptionalPropertyTypes: an optional member's apparent type carries
    // `| undefined` only under EOP=false. (Verter's representation matches EOP=true.)
    const EOP_ON: &str =
        "{ \"compilerOptions\": { \"strict\": true, \"exactOptionalPropertyTypes\": true } }\n";
    const EOP_OFF: &str =
        "{ \"compilerOptions\": { \"strict\": true, \"exactOptionalPropertyTypes\": false } }\n";
    // strictNullChecks: `null` is a distinct arm only under SNC=true; under
    // SNC=false it is absorbed into every type and vanishes from the printed union.
    const SNC_ON: &str = "{ \"compilerOptions\": { \"strict\": true } }\n";
    const SNC_OFF: &str =
        "{ \"compilerOptions\": { \"strict\": true, \"strictNullChecks\": false } }\n";

    let cases = [
        OptionDeliveryCase {
            option: "exactOptionalPropertyTypes",
            fixture: "type Eop = { a?: number };\n",
            probe_rhs: "Eop",
            // The `with`-discriminator config is EOP=OFF (which ADDS `undefined`).
            cfg_with: EOP_OFF,
            cfg_without: EOP_ON,
            discriminator: "undefined",
        },
        OptionDeliveryCase {
            option: "strictNullChecks",
            fixture: "type Snc = string | null;\n",
            probe_rhs: "Snc",
            cfg_with: SNC_ON,
            cfg_without: SNC_OFF,
            discriminator: "null",
        },
    ];

    for case in &cases {
        let Some(with_hover) = hover_probe_under(case.cfg_with, case.fixture, case.probe_rhs).await
        else {
            return; // tsgo absent — skip the whole matrix
        };
        let without_hover = hover_probe_under(case.cfg_without, case.fixture, case.probe_rhs)
            .await
            .expect("tsgo present for `with`, must be present for `without`");

        assert!(
            with_hover.contains(case.discriminator),
            "[{}] hover under cfg_with must contain `{}` (option not applied?); got: {with_hover}",
            case.option,
            case.discriminator
        );
        assert!(
            !without_hover.contains(case.discriminator),
            "[{}] hover under cfg_without must NOT contain `{}` (option not applied?); got: {without_hover}",
            case.option,
            case.discriminator
        );
        assert_ne!(
            with_hover, without_hover,
            "[{}] flipping the option must change the hover — proving tsgo delivered + applied it",
            case.option
        );
    }
}

/// PROOF 4b — `noUncheckedIndexedAccess` delivery, proven via a VALUE probe (the
/// form NUIA actually affects: an element-access EXPRESSION, not an indexed-access
/// type). Hover a `const` bound to an index-signature element access under NUIA=ON
/// vs NUIA=OFF; the `| undefined` arm appears only under NUIA=ON, proving tsgo READ
/// + APPLIED `noUncheckedIndexedAccess` from the delivered tsconfig. Completes the
/// third named option of `oracle_options_delivery_proven`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spike_nuia_delivery_via_value_probe() {
    const NUIA_ON: &str =
        "{ \"compilerOptions\": { \"strict\": true, \"noUncheckedIndexedAccess\": true } }\n";
    const NUIA_OFF: &str =
        "{ \"compilerOptions\": { \"strict\": true, \"noUncheckedIndexedAccess\": false } }\n";
    // The value `nuiaObj["x"]` is an element access through a string index
    // signature — `number | undefined` under NUIA, `number` otherwise.
    const SOURCE: &str =
        "declare const nuiaObj: { [k: string]: number };\nconst probeVal = nuiaObj[\"x\"];\n";
    let probe_off = SOURCE.find("probeVal").expect("find value-probe name");

    async fn hover_value(tsconfig: &str, off: usize) -> Option<String> {
        let (provider, path, _dir) = spawn_with(tsconfig, &[("fixture.ts", SOURCE)]).await?;
        let hover = tokio::time::timeout(
            Duration::from_secs(15),
            provider.get_hover(&path, off as u32),
        )
        .await
        .expect("hover did not time out")
        .expect("hover request ok")
        .expect("hover present at value probe");
        Some(hover.contents)
    }

    let Some(on_hover) = hover_value(NUIA_ON, probe_off).await else {
        return; // tsgo absent — skip
    };
    let off_hover = hover_value(NUIA_OFF, probe_off)
        .await
        .expect("tsgo present for ON, must be present for OFF");

    assert!(
        on_hover.contains("undefined"),
        "NUIA=ON element access must carry `| undefined`; got: {on_hover}"
    );
    assert!(
        !off_hover.contains("undefined"),
        "NUIA=OFF element access must NOT carry `| undefined`; got: {off_hover}"
    );
    assert_ne!(
        on_hover, off_hover,
        "flipping noUncheckedIndexedAccess must change the hover — proving delivery"
    );
}

/// PROOF 5 — the vendored-lib forcing mechanism (the NAMED §Q2 "Env pinning"
/// candidate: `"noLib": true` + an explicit corpus-rooted vendored lib file list).
/// Validated HERMETICALLY (no real bundled lib copied) in two halves:
///
/// - **Part 1 (no leak):** under `noLib: true` with NO lib declaration in the
///   program, a lib-only global (`Array`) is UNRESOLVABLE — tsgo emits a
///   diagnostic. This proves tsgo's NATIVE-bundled libs do NOT leak in under
///   `noLib` (the corpus-hermeticity guarantee `oracle_env_corpus_is_complete`
///   rests on). If the bundled libs leaked, `Array` would resolve and the program
///   would be clean.
/// - **Part 2 (vendored honored):** under `noLib: true` PLUS a vendored minimal
///   `corpus-lib.d.ts` declaring `Array`, the same fixture resolves cleanly —
///   proving the explicit corpus-rooted vendored lib list IS honored.
///
/// Together: `noLib` forces tsgo off its bundled libs AND tsgo consults ONLY the
/// vendored program files — so the generator can drive against a frozen vendored
/// corpus with no native-lib leak.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spike_nolib_forces_off_bundled_libs() {
    // `string` is a primitive keyword (not lib-defined); `Array<T>` is a lib-only
    // global — the discriminator for whether a lib surface is present.
    const FIXTURE: &str = "type UsesArray = Array<string>;\n";

    // Part 1 — noLib, NO lib decl: `Array` must be unresolvable.
    const NOLIB_NO_LIB: &str =
        "{ \"compilerOptions\": { \"noLib\": true }, \"files\": [\"fixture.ts\"] }\n";
    let Some((provider, path, _dir)) = spawn_with(NOLIB_NO_LIB, &[("fixture.ts", FIXTURE)]).await
    else {
        return; // tsgo absent — skip
    };
    let diags = tokio::time::timeout(Duration::from_secs(15), provider.get_diagnostics(&path))
        .await
        .expect("diagnostics did not time out")
        .expect("diagnostics request ok");
    assert!(
        !diags.is_empty(),
        "noLib + no lib decl must FAIL to resolve the lib-only global `Array` \
         (bundled libs must NOT leak under noLib); got {diags:?}"
    );

    // Part 2 — noLib + a VENDORED minimal `Array` decl: must resolve cleanly.
    const VENDORED_LIB: &str = "interface Array<T> { length: number; }\n";
    const NOLIB_WITH_VENDORED: &str =
        "{ \"compilerOptions\": { \"noLib\": true }, \"files\": [\"corpus-lib.d.ts\", \"fixture.ts\"] }\n";
    let Some((provider, path, _dir)) = spawn_with(
        NOLIB_WITH_VENDORED,
        &[("fixture.ts", FIXTURE), ("corpus-lib.d.ts", VENDORED_LIB)],
    )
    .await
    else {
        return;
    };
    let diags = tokio::time::timeout(Duration::from_secs(15), provider.get_diagnostics(&path))
        .await
        .expect("diagnostics did not time out")
        .expect("diagnostics request ok");
    assert!(
        diags.is_empty(),
        "noLib + a vendored `Array` decl must resolve cleanly (vendored lib honored); got {diags:?}"
    );
}
