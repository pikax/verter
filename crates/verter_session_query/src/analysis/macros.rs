//! AST-free reads over analyzed macro records.

use crate::analysis::types::{AnalyzedMacro, AnalyzedMacroKind};

/// Select the props-root binding name (`const props = …`) for a component's
/// `defineProps` / `withDefaults` pair.
///
/// Vue permits at most one `defineProps` per component, so there is no
/// ambiguity: for `const props = withDefaults(defineProps<T>(), { … })` the
/// declarator's binding is recorded on the OUTER `WithDefaults` macro while
/// the INNER `DefineProps` (which carries the prop fields) has no declarator
/// of its own and records `binding_name: None` — a first-match-wins selection
/// over the pair would observe that inner `None` and disarm every props-usage
/// analysis. The expression-statement form (`withDefaults(defineProps<T>(),
/// { … });` with no declarator) correctly yields `None`.
pub fn props_root_binding(macros: &[AnalyzedMacro]) -> Option<&str> {
    macros
        .iter()
        .filter(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
            )
        })
        .find_map(|m| m.binding_name.as_deref())
}
