//! The AUTHORITATIVE hermetic reject-unclassified gate over the committed
//! CSS-manifest corpus (`corpus/fixtures/` + `corpus/goldens/`).
//!
//! Three default-suite tests:
//!
//! 1. [`corpus_inventory_is_bijective_with_manifest`] — the artifact
//!    reconciliation: every manifest case has exactly one committed fixture
//!    plus its client+server goldens, every on-disk fixture/golden maps back
//!    to exactly one case, and no orphan survives.
//! 2. [`no_unexpected_byte_identical_fixtures`] — the byte-identity
//!    tripwire: the ONLY byte-identical fixture groups are the five known
//!    Spread-erases-Target cell-pairs (each re-proven a pure Spread×Target
//!    pair against the typed manifest levels); any NEW duplicate group, and
//!    any stale allowlist row, fails with the offending fixtures named.
//! 3. [`fixture_observations_are_consistent_with_declared_cells`] — the
//!    observation gate: every committed fixture is parsed and lowered through
//!    Verter's PRODUCTION client pipeline under the feature-gated conformance
//!    trace, and the TYPED observations (attribute provenance, matcher
//!    certainties, scoped-element facts, css routing, typed refusal codes)
//!    must be consistent with the fixture's DECLARED manifest cell on every
//!    trace-observable axis. Slugs and paths IDENTIFY artifacts; they never
//!    DETERMINE cells — no axis below is derived from the slug, the path, or
//!    any source-text scan in this test.
//!
//! Plus the committed RED self-tests at the bottom: each mutates a DECLARED
//! cell level in memory and asserts the observation checkers report the
//! contradiction against the real compiled trace, so a checker silently
//! weakened on one of the exercised violation classes fails in-tree.
//!
//! # Which cell axes the trace can OBSERVE (vs structurally implied)
//!
//! | axis (factor)                  | observation                                                                      |
//! |--------------------------------|----------------------------------------------------------------------------------|
//! | `Target` (3)                   | OBSERVED: the subject [`AttrProvenance::name`] for static spellings; for the    |
//! |                                | `Dynamic`/`Spread` uncertainty forms the subject attribute must be ABSENT from  |
//! |                                | the static-attr trace (negative observation).                                   |
//! | `Quoting` (4)                  | OBSERVED: [`AttrQuoting`] on the subject attribute (static spellings only).     |
//! | `TemplateValueRepresentation` (1) | OBSERVED: [`AttrSourceRepresentation`] on the subject attribute for the five |
//! |                                | static spellings; `Dynamic`/`Spread` observe as the static-attr ABSENCE.        |
//! | `MatchOutcome` (8)             | OBSERVED: the per-selector [`MatchCertainty`] rows (`Yes`→Match, `No`→NoMatch,  |
//! |                                | `Maybe`→Maybe), with the two DECLARED-vs-observed conservatisms encoded in      |
//! |                                | [`expected_certainty_pattern`]: a value-reading selector over an uncertainty    |
//! |                                | form observes `Maybe` for a declared Match, and a `:global(…)` row observes the |
//! |                                | fixed `[No]`/unused signature (the emitter keeps it via `has_global`, not the   |
//! |                                | prune verdict).                                                                 |
//! | `CssSource` (6)                | OBSERVED: external ⇒ [`ClientModule::css`] artifact present; injected ⇒ absent  |
//! |                                | (with the style PROVEN present by the recorded matcher run) plus the            |
//! |                                | `<svelte:options css="injected">` options-attribute provenance row.             |
//! | `StructuralKind` (7)           | PARTIALLY OBSERVED: the certainty-row COUNT (plain/combinator 1, pruning/nested |
//! |                                | 2), the pruning second row, the combinator wrap provenance + scoped-element     |
//! |                                | count, and `ScopedCssArtifact::has_global` ⇔ `Global` on external rows. The     |
//! |                                | selector text itself is NOT re-read.                                            |
//! | `ElementRegion` (5)            | PARTIALLY OBSERVED: `SvelteElement` via the scoped `svelte:element` tag fact    |
//! |                                | (positive AND negative direction on scoped rows); `LegacySlot` hosts the same   |
//! |                                | plain scoped `div` inside the slot fallback region (matcher facts + scoped-     |
//! |                                | element observations). `StaticElement`/`Component`/`Block`/`Snippet` all host   |
//! |                                | the same plain scoped `div` — structurally implied.                             |
//! | `SelectorKind` (0)             | NOT trace-observable (selector spans point into the source; re-reading them     |
//! |                                | would be a source scan). Structurally implied.                                  |
//! | `SelectorValueRepresentation` (2) | NOT trace-observable (same reason). Structurally implied.                    |
//!
//! The structurally-implied axes are NOT silently skipped: the inventory
//! bijection ties every fixture to exactly one declared case, and the crate's
//! own `corpus_matches_committed` guard (`src/generate_tests.rs`) pins every
//! fixture's bytes to the manifest rendering of that declared cell — a
//! coverage note, not a gap this test papers over.
//!
//! Disposition partitions are fully observed as typed outcomes: a Supported
//! cell must COMPILE (and yield the trace facts above); the refusal partition
//! is UNINHABITED (the empty `RefusalKind` match keeps the rail closed); an
//! `OracleRejected(CssNestingSelectorInvalidPlacement)` cell must fail closed
//! in Verter's css analysis with the SAME official diagnostic code
//! (`css_nesting_selector_invalid_placement`) — observed BEFORE lowering, so
//! its trace is legitimately empty.
//!
//! Hermetic: committed files + the pinned in-process manifest + in-process
//! Verter lowering only. No node, no live official compiler.
//!
//! [`AttrProvenance::name`]: verter_compiler::svelte::runtime::conformance_trace::AttrProvenance
//! [`StyleSelectorUnsupported`]: UnsupportedSvelteRuntimeSurface::StyleSelectorUnsupported

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{assert_no_violations, case_runtime_options, corpus_root, expected_certainty_pattern};
use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::conformance_trace::{
    compile_client_with_conformance_trace, AttrQuoting, AttrSourceRepresentation, ConformanceTrace,
    MatchCertainty,
};
use verter_compiler::svelte::runtime::{
    ClientCompileError, ClientModule, UnsupportedSvelteRuntimeSurface,
};
use verter_svelte_conformance::manifest::{manifest, ManifestCase};
use verter_svelte_conformance::model::{
    CompileTarget, CssSource, DiagnosticKind, Disposition, ElementRegion, MatchOutcome, Quoting,
    StructuralKind, Target, TemplateValueRepresentation,
};

/// The committed corpus size this gate runs over (one fixture per manifest
/// case) — the shared test-side pin (`common/case_count.rs`). A manifest
/// change legitimately moves it, in lockstep across every conformance gate.
use common::case_count::CASE_COUNT;

// ---------------------------------------------------------------------------
// Inventory bijection
// ---------------------------------------------------------------------------

/// The backend wire spelling of a golden file segment.
fn backend_wire(backend: CompileTarget) -> &'static str {
    backend.id()
}

#[test]
fn corpus_inventory_is_bijective_with_manifest() {
    let manifest = manifest();
    let root = corpus_root();
    let mut violations: Vec<String> = Vec::new();

    assert_eq!(
        manifest.cases().len(),
        CASE_COUNT,
        "manifest case inventory moved; re-pin CASE_COUNT together with the corpus"
    );
    let case_slugs: BTreeSet<&str> = manifest
        .cases()
        .iter()
        .map(|case| case.slug.as_str())
        .collect();
    assert_eq!(
        case_slugs.len(),
        manifest.cases().len(),
        "manifest slugs must be unique (one committed fixture per case)"
    );

    // Fixtures on disk ↔ manifest cases.
    let mut fixture_stems: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("fixtures")).expect("committed fixtures dir reads") {
        let entry = entry.expect("fixture dir entry reads");
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            violations.push(format!("non-UTF-8 entry under fixtures/: {name:?}"));
            continue;
        };
        if entry.path().is_dir() {
            violations.push(format!("unexpected directory under fixtures/: {name}"));
            continue;
        }
        let Some(stem) = name.strip_suffix(".svelte") else {
            violations.push(format!("non-fixture file under fixtures/: {name}"));
            continue;
        };
        if !case_slugs.contains(stem) {
            violations.push(format!(
                "unclassified fixture (maps to NO manifest case): fixtures/{name}"
            ));
            continue;
        }
        fixture_stems.insert(stem.to_string());
    }
    for slug in &case_slugs {
        if !fixture_stems.contains(*slug) {
            violations.push(format!(
                "manifest case without a committed fixture: fixtures/{slug}.svelte"
            ));
        }
    }

    // Goldens on disk ↔ (case × backend), with per-file identity integrity.
    let mut golden_names: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("goldens")).expect("committed goldens dir reads") {
        let entry = entry.expect("golden dir entry reads");
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            violations.push(format!("non-UTF-8 entry under goldens/: {name:?}"));
            continue;
        };
        if entry.path().is_dir() {
            violations.push(format!("unexpected directory under goldens/: {name}"));
            continue;
        }
        let stem_backend = name
            .strip_suffix(".json")
            .and_then(|stem| stem.rsplit_once('.'));
        let Some((stem, backend)) = stem_backend else {
            violations.push(format!("non-golden file under goldens/: {name}"));
            continue;
        };
        if !matches!(backend, "client" | "server") {
            violations.push(format!(
                "orphan golden (unknown backend segment): goldens/{name}"
            ));
            continue;
        }
        if !case_slugs.contains(stem) {
            violations.push(format!(
                "orphan golden (maps to NO manifest case): goldens/{name}"
            ));
            continue;
        }
        // Identity integrity: the payload's own identity fields must agree
        // with the artifact name (a golden written under the wrong name is an
        // orphan in disguise).
        let text = std::fs::read_to_string(entry.path()).expect("committed golden reads");
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => {
                if value.get("slug").and_then(serde_json::Value::as_str) != Some(stem) {
                    violations.push(format!("golden slug field mismatch: goldens/{name}"));
                }
                if value.get("backend").and_then(serde_json::Value::as_str) != Some(backend) {
                    violations.push(format!("golden backend field mismatch: goldens/{name}"));
                }
            }
            Err(error) => {
                violations.push(format!("golden is not valid JSON: goldens/{name}: {error}"))
            }
        }
        golden_names.insert(name.to_string());
    }
    for case in manifest.cases() {
        for backend in case.backends {
            let expected = format!("{}.{}.json", case.slug, backend_wire(backend));
            if !golden_names.contains(&expected) {
                violations.push(format!("missing golden: goldens/{expected}"));
            }
        }
    }

    if violations.is_empty() {
        assert_eq!(fixture_stems.len(), CASE_COUNT, "fixture count");
        assert_eq!(golden_names.len(), CASE_COUNT * 2, "golden count");
    }
    assert_no_violations("corpus inventory bijection", &violations);
}

// ---------------------------------------------------------------------------
// Byte-identity tripwire
// ---------------------------------------------------------------------------

/// The KNOWN byte-identical fixture pairs — the documented
/// Spread-erases-Target degeneracy, and nothing else: the model's fixture
/// renderer (`subject_attr()` in `src/model.rs`) spells every
/// `template=Spread` value as `{...rest}` regardless of the declared
/// [`Target`], so a cell-pair differing ONLY in the Target axis under
/// `Spread` legitimately renders one shared fixture body (the two cells are
/// semantically identical — the behavior is tested once, not untested). The
/// material fix — modeling `Target` as degenerate under `template=Spread`
/// and collapsing the axis in the covering-array constraint so the corpus
/// becomes injective — is captured as a future refinement in
/// `docs/better-implementation/T1b-runtime-codegen-manifest.md` §4.3.
///
/// Slugs here IDENTIFY the allowlisted artifacts; the Spread×Target
/// semantics of every pair are re-proven against the TYPED manifest levels
/// in [`no_unexpected_byte_identical_fixtures`], so a stale or mis-scoped
/// allowlist row fails the gate rather than silently widening it.
const KNOWN_SPREAD_TARGET_SHARED_FIXTURES: &[&[&str]] = &[
    &[
        "type-spread-lit-attr-q-blk-ext-plain-n",
        "type-spread-lit-cls-q-blk-ext-plain-n",
    ],
    &[
        "type-spread-lit-attr-q-comp-ext-plain-n",
        "type-spread-lit-cls-q-comp-ext-plain-n",
        "type-spread-lit-id-q-comp-ext-plain-n",
    ],
    &[
        "type-spread-lit-attr-q-el-ext-plain-m",
        "type-spread-lit-cls-q-el-ext-plain-m",
    ],
    &[
        "type-spread-lit-attr-q-slot-ext-plain-n",
        "type-spread-lit-cls-q-slot-ext-plain-n",
        "type-spread-lit-id-q-slot-ext-plain-n",
    ],
    &[
        "type-spread-lit-cls-q-snip-ext-plain-m",
        "type-spread-lit-id-q-snip-ext-plain-m",
    ],
    &[
        "type-spread-lit-attr-q-snip-ext-plain-n",
        "type-spread-lit-id-q-snip-ext-plain-n",
    ],
    &[
        "type-spread-lit-cls-q-el-ext-plain-n",
        "type-spread-lit-id-q-el-ext-plain-n",
    ],
];

/// The Target factor's slug segment (a case slug is the nine dash-free level
/// ids joined by `-`, in factor order — `model::slug`).
const TARGET_SLUG_SEGMENT: usize = 3;

/// The slug segment count of a case slug (one id per factor).
const SLUG_SEGMENT_COUNT: usize = 9;

#[test]
fn no_unexpected_byte_identical_fixtures() {
    let manifest = manifest();
    let root = corpus_root();
    let mut violations: Vec<String> = Vec::new();

    // The allowlist itself must stay EXACT — every row must be precisely the
    // documented Spread-erases-Target degeneracy, in both spellings:
    //
    // - slug shape: both slugs in the `type-spread-lit-*` family, differing
    //   ONLY in the Target segment (`attr` / `cls` / `id`);
    // - typed semantics: both slugs resolve to manifest cases declaring
    //   `template=Spread`, whose levels differ ONLY in the `target` axis.
    for group in KNOWN_SPREAD_TARGET_SHARED_FIXTURES {
        assert!(
            group.len() >= 2,
            "an allowlisted group needs at least two slugs: {group:?}"
        );
        for slug in *group {
            if !slug.starts_with("type-spread-lit-") {
                violations.push(format!(
                    "allowlist row outside the documented `type-spread-lit-*` family: {slug}"
                ));
            }
        }
        // Every PAIR within the group must be the pure Spread×Target
        // degeneracy — both spellings (the slug shape and the typed levels).
        for (i, slug_a) in group.iter().enumerate() {
            for slug_b in &group[i + 1..] {
                let segments_a: Vec<&str> = slug_a.split('-').collect();
                let segments_b: Vec<&str> = slug_b.split('-').collect();
                if segments_a.len() != SLUG_SEGMENT_COUNT || segments_b.len() != SLUG_SEGMENT_COUNT
                {
                    violations.push(format!(
                        "allowlisted slugs are not nine-segment case slugs: {slug_a} / {slug_b}"
                    ));
                } else {
                    for (index, (segment_a, segment_b)) in
                        segments_a.iter().zip(&segments_b).enumerate()
                    {
                        if index == TARGET_SLUG_SEGMENT {
                            let target_ids = ["attr", "cls", "id"];
                            if segment_a == segment_b
                                || !target_ids.contains(segment_a)
                                || !target_ids.contains(segment_b)
                            {
                                violations.push(format!(
                                    "allowlisted pair must differ in the Target segment \
                                     (`attr`/`cls`/`id`): {slug_a} / {slug_b}"
                                ));
                            }
                        } else if segment_a != segment_b {
                            violations.push(format!(
                                "allowlisted pair differs outside the Target segment \
                                 (segment {index}): {slug_a} / {slug_b}"
                            ));
                        }
                    }
                }
                match (
                    manifest.case_for_slug(slug_a),
                    manifest.case_for_slug(slug_b),
                ) {
                    (Some(case_a), Some(case_b)) => {
                        for case in [case_a, case_b] {
                            if case.levels.template_value != TemplateValueRepresentation::Spread {
                                violations.push(format!(
                                    "allowlisted case is not a Spread cell (template={:?}): {}",
                                    case.levels.template_value, case.slug
                                ));
                            }
                        }
                        let mut target_neutral = case_a.levels;
                        target_neutral.target = case_b.levels.target;
                        if case_a.levels.target == case_b.levels.target
                            || target_neutral != case_b.levels
                        {
                            violations.push(format!(
                                "allowlisted pair is not a pure Spread×Target cell-pair \
                                 (declared levels must differ ONLY in `target`): \
                                 {slug_a} / {slug_b}"
                            ));
                        }
                    }
                    _ => violations.push(format!(
                        "stale allowlist: pair references a slug with NO manifest case: \
                         {slug_a} / {slug_b}"
                    )),
                }
            }
        }
    }

    // Group EVERY committed fixture by checkout-EOL-normalized content
    // (CRLF ⇒ LF, so the grouping is stable across autocrlf checkouts).
    // Non-`.svelte` entries are the bijection gate's findings, not dupes.
    let mut fixtures_by_content: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in std::fs::read_dir(root.join("fixtures")).expect("committed fixtures dir reads") {
        let entry = entry.expect("fixture dir entry reads");
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let Some(stem) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".svelte"))
        else {
            continue;
        };
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("committed fixture {} reads: {error}", path.display()))
            .replace("\r\n", "\n");
        fixtures_by_content
            .entry(content)
            .or_default()
            .insert(stem.to_string());
    }

    let allowed: BTreeSet<BTreeSet<String>> = KNOWN_SPREAD_TARGET_SHARED_FIXTURES
        .iter()
        .map(|pair| pair.iter().map(ToString::to_string).collect())
        .collect();
    let duplicate_groups: BTreeSet<BTreeSet<String>> = fixtures_by_content
        .into_values()
        .filter(|slugs| slugs.len() >= 2)
        .collect();

    // Any duplicate group beyond the documented degeneracy is a NEW collapse
    // (a non-Spread collapse, or a new Spread pair) — name it; extending the
    // allowlist requires re-proving the new pair Spread×Target above.
    for group in &duplicate_groups {
        if !allowed.contains(group) {
            violations.push(format!(
                "unexpected byte-identical fixture group (NOT the documented \
                 Spread-erases-Target set): {:?}",
                group.iter().collect::<Vec<_>>()
            ));
        }
    }
    // Any allowlisted pair that is NOT byte-identical on disk is a stale
    // allowlist row — the degeneracy it documents no longer exists.
    for pair in &allowed {
        if !duplicate_groups.contains(pair) {
            violations.push(format!(
                "stale allowlist: pair is no longer byte-identical on disk: {:?}",
                pair.iter().collect::<Vec<_>>()
            ));
        }
    }

    assert_no_violations("fixture byte-identity tripwire", &violations);
}

// ---------------------------------------------------------------------------
// Observation → declared-cell consistency
// ---------------------------------------------------------------------------

/// The authored attribute name of a target (mirrors the model's rendering
/// contract; used to LOCATE the subject among typed provenance rows).
fn target_attr_name(target: Target) -> &'static str {
    match target {
        Target::Class => "class",
        Target::Id => "id",
        Target::Attr => "data-x",
    }
}

/// One typed static-attr provenance signature.
type AttrSignature = (String, AttrQuoting, Option<AttrSourceRepresentation>);

/// The trace-level quoting a declared [`Quoting`] level must observe as.
fn expected_quoting(quoting: Quoting) -> AttrQuoting {
    match quoting {
        Quoting::Quoted => AttrQuoting::Quoted,
        Quoting::Unquoted => AttrQuoting::Unquoted,
        Quoting::Boolean => AttrQuoting::BooleanValueless,
    }
}

/// The trace-level source representation a declared STATIC template spelling
/// must observe as (`None` = the declared level is an uncertainty form with
/// no static spelling).
fn expected_representation(
    template: TemplateValueRepresentation,
) -> Option<AttrSourceRepresentation> {
    match template {
        TemplateValueRepresentation::Literal => Some(AttrSourceRepresentation::Literal),
        TemplateValueRepresentation::HtmlNamedEntity => {
            Some(AttrSourceRepresentation::HtmlNamedEntity)
        }
        TemplateValueRepresentation::HtmlDecimalEntity => {
            Some(AttrSourceRepresentation::HtmlDecimalEntity)
        }
        TemplateValueRepresentation::HtmlHexEntity => Some(AttrSourceRepresentation::HtmlHexEntity),
        TemplateValueRepresentation::MixedLiteralEntity => Some(AttrSourceRepresentation::Mixed),
        TemplateValueRepresentation::Dynamic | TemplateValueRepresentation::Spread => None,
    }
}

/// Verify the static-attribute provenance rows against the declared cell.
///
/// The pool starts as EVERY recorded provenance row; the two declared
/// bystanders are consumed first (each must itself be present — they are
/// observations of the `css_source` / `structural` axes):
///
/// - `Injected` ⇒ one `css` quoted-literal row (`<svelte:options css="injected"`).
/// - `Combinator` ⇒ one `class` quoted-literal row (the `.wrap` host).
///
/// Then the SUBJECT: a static spelling must witness the exact declared
/// `(target, quoting, representation)` triple among the remaining rows; an
/// uncertainty form (`Dynamic` / `Spread`) must observe the subject
/// attribute's ABSENCE from the remaining rows (its value is an expression /
/// spread — not a static attribute).
fn check_attr_provenance(
    case: &ManifestCase,
    trace: &ConformanceTrace,
    violations: &mut Vec<String>,
) {
    let levels = case.levels;
    let mut pool: Vec<AttrSignature> = trace
        .static_attrs
        .iter()
        .map(|attr| (attr.name.clone(), attr.quoting, attr.representation))
        .collect();

    let consume_bystander = |pool: &mut Vec<AttrSignature>,
                             name: &str,
                             what: &str,
                             violations: &mut Vec<String>| {
        let signature: AttrSignature = (
            name.to_string(),
            AttrQuoting::Quoted,
            Some(AttrSourceRepresentation::Literal),
        );
        match pool.iter().position(|row| *row == signature) {
            Some(index) => {
                pool.remove(index);
            }
            None => violations.push(format!(
                "{}: declared {what} but its quoted-literal `{name}` provenance row was not observed",
                case.slug
            )),
        }
    };
    if levels.css_source == CssSource::Injected {
        consume_bystander(&mut pool, "css", "css=\"injected\"", violations);
    }
    if levels.structural == StructuralKind::Combinator {
        consume_bystander(&mut pool, "class", "a combinator `.wrap` host", violations);
    }

    let subject_name = target_attr_name(levels.target);
    match expected_representation(levels.template_value) {
        None => {
            if pool.iter().any(|(name, _, _)| name == subject_name) {
                violations.push(format!(
                    "{}: declared template value {:?} (an uncertainty form) but the subject \
                     `{subject_name}` was observed as a STATIC attribute",
                    case.slug, levels.template_value
                ));
            }
        }
        Some(expected_repr) => {
            let expected_repr = if levels.quoting == Quoting::Boolean {
                // A valueless attribute has no value text to represent.
                None
            } else {
                Some(expected_repr)
            };
            let signature: AttrSignature = (
                subject_name.to_string(),
                expected_quoting(levels.quoting),
                expected_repr,
            );
            if !pool.contains(&signature) {
                violations.push(format!(
                    "{}: no static-attr observation witnesses the declared subject \
                     (name=`{subject_name}`, quoting={:?}, representation={:?}); observed rows: {:?}",
                    case.slug, levels.quoting, levels.template_value, pool
                ));
            }
        }
    }
}

/// Verify one SUPPORTED case's compiled module + trace against its declared
/// cell (certainty pattern, css routing, `has_global`, scoped-element facts).
fn check_supported(
    case: &ManifestCase,
    module: &ClientModule,
    trace: &ConformanceTrace,
    violations: &mut Vec<String>,
) {
    let levels = case.levels;

    // Exactly one <style> matcher run must have been observed — the
    // non-vacuity anchor for every matcher-fact assertion below.
    if trace.style_matches.len() != 1 {
        violations.push(format!(
            "{}: expected exactly one style matcher run, observed {}",
            case.slug,
            trace.style_matches.len()
        ));
        return;
    }
    let style = &trace.style_matches[0];

    // Declared outcome (+ structural composition) vs observed certainties.
    let observed: Vec<MatchCertainty> = style
        .selector_certainties
        .iter()
        .map(|fact| fact.certainty)
        .collect();
    let expected = expected_certainty_pattern(case);
    if observed != expected {
        violations.push(format!(
            "{}: declared outcome {:?} (structural {:?}) expects certainty pattern {expected:?}, \
             observed {observed:?}",
            case.slug, levels.outcome, levels.structural
        ));
    }

    // Declared css source vs the typed module artifact routing.
    match levels.css_source {
        CssSource::External => match &module.css {
            None => violations.push(format!(
                "{}: declared External css but the compile produced no external css artifact",
                case.slug
            )),
            Some(css) => {
                let expect_global = levels.structural == StructuralKind::Global;
                if css.has_global != expect_global {
                    violations.push(format!(
                        "{}: declared structural {:?} expects has_global={expect_global}, \
                         observed {}",
                        case.slug, levels.structural, css.has_global
                    ));
                }
            }
        },
        CssSource::Injected => {
            if module.css.is_some() {
                violations.push(format!(
                    "{}: declared Injected css but the compile produced an external css artifact",
                    case.slug
                ));
            }
        }
    }

    // Scoped-element facts: the subject is scoped exactly when ANY top-level
    // selector row is kept (non-`No` — a fail-open `Maybe` row scopes too),
    // except the never-scoping `:global(…)` row; the combinator wrap host
    // adds one more scoped element.
    let subject_kept = expected
        .iter()
        .any(|certainty| *certainty != MatchCertainty::No)
        && levels.structural != StructuralKind::Global;
    let expected_scoped = match (subject_kept, levels.structural) {
        (false, _) => 0,
        (true, StructuralKind::Combinator) => 2,
        (true, _) => 1,
    };
    if style.scoped_elements.len() != expected_scoped {
        violations.push(format!(
            "{}: expected {expected_scoped} scoped element(s), observed {:?}",
            case.slug,
            style
                .scoped_elements
                .iter()
                .map(|element| element.tag.as_str())
                .collect::<Vec<_>>()
        ));
    }
    if subject_kept {
        let dynamic_element = levels.region == ElementRegion::SvelteElement;
        let has_dynamic_tag = style
            .scoped_elements
            .iter()
            .any(|element| element.tag == "svelte:element");
        if dynamic_element && !has_dynamic_tag {
            violations.push(format!(
                "{}: declared region SvelteElement but no scoped `svelte:element` fact observed",
                case.slug
            ));
        }
        if !dynamic_element && has_dynamic_tag {
            violations.push(format!(
                "{}: declared region {:?} but a scoped `svelte:element` fact was observed",
                case.slug, levels.region
            ));
        }
        if !dynamic_element
            && !style
                .scoped_elements
                .iter()
                .any(|element| element.tag == "div")
        {
            violations.push(format!(
                "{}: the scoped subject `div` fact was not observed",
                case.slug
            ));
        }
    }
}

#[test]
fn fixture_observations_are_consistent_with_declared_cells() {
    let manifest = manifest();
    let root = corpus_root();
    let mut violations: Vec<String> = Vec::new();
    let (mut supported_seen, mut oracle_rejected_seen) = (0usize, 0usize);

    for case in manifest.cases() {
        let path = root.join("fixtures").join(format!("{}.svelte", case.slug));
        let Ok(source) = std::fs::read_to_string(&path) else {
            // The bijection gate reports the missing fixture precisely.
            violations.push(format!("{}: committed fixture unreadable", case.slug));
            continue;
        };

        let allocator = Allocator::default();
        let parsed = parse_svelte(&source);
        let options = case_runtime_options(case);
        let (result, trace) =
            compile_client_with_conformance_trace(&source, &parsed, &options, &allocator, false);

        match case.disposition {
            Disposition::Supported => {
                supported_seen += 1;
                match &result {
                    Ok(module) => check_supported(case, module, &trace, &mut violations),
                    Err(error) => violations.push(format!(
                        "{}: declared Supported but Verter refused: {error:?}",
                        case.slug
                    )),
                }
                check_attr_provenance(case, &trace, &mut violations);
            }
            // The refusal vocabulary is UNINHABITED — a declared Refused row is
            // impossible by construction (the empty match proves the rail stays
            // closed; a future refusal kind must land with its own typed
            // observation arm here).
            Disposition::Refused(kind) => match kind {},
            Disposition::OracleRejected(kind) => {
                oracle_rejected_seen += 1;
                // Exhaustive: a new diagnostic kind must land with its own
                // typed observation arm. Verter's own css analysis rejects
                // this construct with the SAME official code, BEFORE lowering
                // — the typed error IS the observation and the trace is
                // legitimately empty.
                let DiagnosticKind::CssNestingSelectorInvalidPlacement = kind;
                match &result {
                    Err(ClientCompileError::Unsupported(
                        UnsupportedSvelteRuntimeSurface::StyleCssAnalysis { code, .. },
                    )) if *code == "css_nesting_selector_invalid_placement" => {}
                    other => violations.push(format!(
                        "{}: declared OracleRejected({kind:?}) expects the typed \
                         `css_nesting_selector_invalid_placement` css-analysis refusal, \
                         observed {other:?}",
                        case.slug
                    )),
                }
                if !(trace.static_attrs.is_empty() && trace.style_matches.is_empty()) {
                    violations.push(format!(
                        "{}: a pre-lowering css-analysis reject must observe an empty trace",
                        case.slug
                    ));
                }
            }
            Disposition::Invalid(kind) => violations.push(format!(
                "{}: the manifest selected an Invalid({kind:?}) row as a case",
                case.slug
            )),
        }
    }

    // Non-vacuity: the gate must have exercised every INHABITED disposition
    // partition over the full committed corpus (the refusal partition is
    // uninhabited by construction — the empty `RefusalKind` match above).
    assert_eq!(
        supported_seen + oracle_rejected_seen,
        CASE_COUNT,
        "every committed case must be observed"
    );
    assert!(supported_seen > 0, "no Supported case was observed");
    assert!(
        oracle_rejected_seen > 0,
        "no OracleRejected case was observed"
    );
    assert_no_violations("observation → declared-cell consistency", &violations);
}

// ---------------------------------------------------------------------------
// Committed RED self-tests: each mutates the DECLARED cell in memory and
// asserts the observation checkers report the contradiction against the REAL
// compiled trace — a checker silently weakened on one of the violation
// classes exercised below fails IN-TREE, without any out-of-tree plant
// recipe (unexercised classes rely on the out-of-tree plant recipes). (Slugs
// still only locate the fixture; the mutated axis is always a typed level,
// never a path.)
// ---------------------------------------------------------------------------

/// Compile one committed fixture through Verter's client pipeline.
fn compile_fixture(
    case: &ManifestCase,
) -> (Result<ClientModule, ClientCompileError>, ConformanceTrace) {
    let path = corpus_root()
        .join("fixtures")
        .join(format!("{}.svelte", case.slug));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("committed fixture {} reads: {error}", path.display()));
    let allocator = Allocator::default();
    let parsed = parse_svelte(&source);
    let options = case_runtime_options(case);
    compile_client_with_conformance_trace(&source, &parsed, &options, &allocator, false)
}

/// The first manifest case satisfying `predicate` (deterministic: case order
/// is the manifest's ascending row order).
fn find_case(predicate: impl Fn(&ManifestCase) -> bool) -> &'static ManifestCase {
    manifest()
        .cases()
        .iter()
        .find(|case| predicate(case))
        .expect("the committed manifest carries a case for this self-test predicate")
}

/// A declared quoting the fixture does not spell is reported by the
/// attribute-provenance checker (the subject witness is the TYPED
/// `(target, quoting, representation)` triple, so a single flipped level
/// breaks it).
#[test]
fn self_test_contradicted_declared_quoting_is_detected() {
    let case = find_case(|case| {
        case.disposition == Disposition::Supported
            && case.levels.template_value == TemplateValueRepresentation::Literal
            && case.levels.quoting == Quoting::Quoted
    });
    let (_result, trace) = compile_fixture(case);

    let mut violations = Vec::new();
    check_attr_provenance(case, &trace, &mut violations);
    assert!(
        violations.is_empty(),
        "control: the declared cell must be witnessed by the real trace"
    );

    let mut contradicted = case.clone();
    contradicted.levels.quoting = Quoting::Unquoted;
    check_attr_provenance(&contradicted, &trace, &mut violations);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("no static-attr observation witnesses")),
        "a contradicted declared quoting must fail the provenance witness: {violations:?}"
    );
}

/// A declared outcome the real matcher run contradicts is reported by the
/// supported-case checker (certainty pattern + scoped-element facts both
/// derive from the DECLARED cell).
#[test]
fn self_test_contradicted_declared_outcome_is_detected() {
    let case = find_case(|case| {
        case.disposition == Disposition::Supported
            && case.levels.template_value == TemplateValueRepresentation::Literal
            && case.expected_outcome == MatchOutcome::Match
    });
    let (result, trace) = compile_fixture(case);
    let module = result.expect("a supported fixture compiles");

    let mut violations = Vec::new();
    check_supported(case, &module, &trace, &mut violations);
    assert!(
        violations.is_empty(),
        "control: the declared cell must be consistent with the real observations"
    );

    let mut contradicted = case.clone();
    contradicted.levels.outcome = MatchOutcome::NoMatch;
    contradicted.expected_outcome = MatchOutcome::NoMatch;
    check_supported(&contradicted, &module, &trace, &mut violations);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("expects certainty pattern")),
        "a contradicted declared outcome must fail the certainty comparison: {violations:?}"
    );
}

/// A declared css source the compiled module contradicts is reported by the
/// supported-case checker (External ⇔ the typed external artifact exists).
#[test]
fn self_test_contradicted_declared_css_source_is_detected() {
    let case = find_case(|case| {
        case.disposition == Disposition::Supported
            && case.levels.css_source == CssSource::External
            && case.expected_outcome == MatchOutcome::Match
    });
    let (result, trace) = compile_fixture(case);
    let module = result.expect("a supported fixture compiles");

    let mut violations = Vec::new();
    check_supported(case, &module, &trace, &mut violations);
    assert!(
        violations.is_empty(),
        "control: the declared cell must be consistent with the real observations"
    );

    let mut contradicted = case.clone();
    contradicted.levels.css_source = CssSource::Injected;
    check_supported(&contradicted, &module, &trace, &mut violations);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("declared Injected css but the compile produced")),
        "a contradicted declared css source must fail the artifact routing check: \
         {violations:?}"
    );
}
