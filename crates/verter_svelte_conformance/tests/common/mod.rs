//! Shared helpers for the conformance crate's integration suites
//! (`reject_unclassified`, `oracle_differential`): the committed-corpus root,
//! the golden-mirroring Verter compile options, the declared-cell certainty
//! expectations, and the violation-list renderer.
//!
//! Everything here is derived from TYPED manifest/model/trace data — slugs and
//! paths identify artifacts, they never determine semantics.

/// The shared committed-corpus size pin (suites that need ONLY the pin
/// include `common/case_count.rs` directly via `#[path]`).
pub mod case_count;

use std::path::{Path, PathBuf};

use verter_compiler::svelte::runtime::conformance_trace::MatchCertainty;
use verter_compiler::svelte::runtime::SvelteRuntimeOptions;
use verter_svelte_conformance::manifest::ManifestCase;
use verter_svelte_conformance::model::{
    MatchOutcome, SelectorKind, StructuralKind, Target, TemplateValueRepresentation,
};

/// The conformance crate's committed corpus root.
pub fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

/// Render a violation list into one actionable panic message.
pub fn assert_no_violations(gate: &str, violations: &[String]) {
    assert!(
        violations.is_empty(),
        "{gate}: {} violation(s):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

/// The component `name` compile option the golden generator derives from a
/// fixture slug (the filename stem, JS-identifier-sanitized): every character
/// outside `[A-Za-z0-9_$]` becomes `_`, and a stem that does not start with an
/// identifier-start character gains a leading `_`.
pub fn component_name_for(slug: &str) -> String {
    let sanitized: String = slug
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let starts_like_identifier = sanitized
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_' || ch == '$');
    if starts_like_identifier {
        sanitized
    } else {
        format!("_{sanitized}")
    }
}

/// The Verter runtime options mirroring the committed goldens' compile
/// options: `filename: "<slug>.svelte"` (or the declared filename-undefined
/// fallback), `name: componentNameFor(slug)`, plus the typed per-case options.
pub fn case_runtime_options(case: &ManifestCase) -> SvelteRuntimeOptions {
    SvelteRuntimeOptions {
        filename: if case.compile_options.filename_undefined {
            None
        } else {
            Some(format!("{}.svelte", case.slug))
        },
        name: Some(component_name_for(&case.slug)),
        custom_element: case.compile_options.custom_element,
        ..Default::default()
    }
}

/// Whether the declared selector kind reads the subject ATTRIBUTE VALUE (the
/// nesting selector reads it through its target-adapted parent rule; type /
/// universal selectors read the element, not the attribute).
pub fn selector_reads_value(kind: SelectorKind) -> bool {
    matches!(
        kind,
        SelectorKind::Class | SelectorKind::Id | SelectorKind::Attribute | SelectorKind::Nesting
    )
}

/// The certainty the SUBJECT selector must observe for a declared cell.
///
/// `Yes`→Match / `No`→NoMatch / `Maybe`→Maybe, with ONE encoded conservatism:
/// a declared Match whose subject value is an UNCERTAINTY form (`Dynamic` /
/// `Spread`) read by a value-reading selector observes `Maybe` — the matcher
/// keeps it fail-open rather than proving the match (the grounded official
/// verdict stays Match; the production used/scoped projection is identical).
///
/// That mapping is the PRINCIPLED tri-state for a runtime-variable value,
/// not a test exemption: it is typed on the uncertainty forms themselves
/// (`Dynamic` / `Spread`), and it mirrors the official
/// `get_possible_values` semantics, where an enumerated POSSIBLE value is
/// never a proof the matching branch is taken at runtime — so
/// runtime-variable ⇒ unproven ⇒ `Maybe`. The certainty stays COMPUTED,
/// never blanket: a dynamic value whose enumerated possible values provably
/// exclude the selector still observes `No`, and a static Match member still
/// observes `Yes` — both directions are pinned by the per-family
/// decode-divergent negative controls in `tests/metamorphic.rs` (a
/// treats-every-expression-as-unknown matcher fails there).
pub fn expected_subject_certainty(
    kind: SelectorKind,
    template: TemplateValueRepresentation,
    outcome: MatchOutcome,
) -> MatchCertainty {
    let value_uncertain = matches!(
        template,
        TemplateValueRepresentation::Dynamic | TemplateValueRepresentation::Spread
    ) && selector_reads_value(kind);
    match outcome {
        MatchOutcome::Match => {
            if value_uncertain {
                MatchCertainty::Maybe
            } else {
                MatchCertainty::Yes
            }
        }
        MatchOutcome::NoMatch => MatchCertainty::No,
        MatchOutcome::Maybe => MatchCertainty::Maybe,
    }
}

/// The full per-top-level-selector certainty pattern (prune visit order) a
/// declared SUPPORTED cell must observe.
///
/// - `Plain` / `Combinator`: the subject row only.
/// - `Pruning`: the subject row, then the `.unused-prune` row — provably
///   unused (`No`) unless the subject can carry an OPEN class set: a `Spread`
///   subject (any attribute may appear) or an open `Dynamic` value on the
///   `class` target (the declared-Maybe form; the certain-outcome dynamic
///   conditionals enumerate to a closed set, which keeps the prune provable).
/// - `Nested`: both rows carry the subject verdict (`SEL { &:hover {…} }`;
///   for the nesting kind the parent carries the target-reading selector).
/// - `Global`: the fixed `[No]` never-pruned signature — a `:global(…)`
///   selector is not matched against template elements; the emitter keeps it
///   via the `has_global` routing observed separately.
pub fn expected_certainty_pattern(case: &ManifestCase) -> Vec<MatchCertainty> {
    let levels = case.levels;
    let subject =
        expected_subject_certainty(levels.selector_kind, levels.template_value, levels.outcome);
    match levels.structural {
        StructuralKind::Plain | StructuralKind::Combinator => vec![subject],
        StructuralKind::Pruning => {
            let open_class_set = levels.template_value == TemplateValueRepresentation::Spread
                || (levels.template_value == TemplateValueRepresentation::Dynamic
                    && levels.target == Target::Class
                    && levels.outcome == MatchOutcome::Maybe);
            let second = if open_class_set {
                MatchCertainty::Maybe
            } else {
                MatchCertainty::No
            };
            vec![subject, second]
        }
        StructuralKind::Nested => vec![subject, subject],
        StructuralKind::Global => vec![MatchCertainty::No],
    }
}
