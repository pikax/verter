use verter_language::{
    compare_language_diagnostic_fields, LanguageDiagnosticOrderKey, LanguageDiagnosticSeverity,
};
use verter_span::Span;

/// Parser-owned candidate carrying discovery order and the normative diagnostic tie-breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParseDefectCandidate<T> {
    pub(crate) encounter_order: u32,
    pub(crate) span: Span,
    pub(crate) code: &'static str,
    pub(crate) payload: T,
}

/// Select the earliest provable parser defect, using the shared diagnostic key
/// only when two retained facts have the same discovery order.
pub(crate) fn select_parse_defect<T: Copy>(
    candidates: impl IntoIterator<Item = ParseDefectCandidate<T>>,
) -> Option<ParseDefectCandidate<T>> {
    candidates.into_iter().min_by(compare_candidates)
}

fn compare_candidates<T>(
    left: &ParseDefectCandidate<T>,
    right: &ParseDefectCandidate<T>,
) -> std::cmp::Ordering {
    left.encounter_order
        .cmp(&right.encounter_order)
        .then_with(|| {
            compare_language_diagnostic_fields(
                LanguageDiagnosticOrderKey::new(
                    left.span,
                    LanguageDiagnosticSeverity::Error,
                    left.code,
                    &[],
                ),
                LanguageDiagnosticOrderKey::new(
                    right.span,
                    LanguageDiagnosticSeverity::Error,
                    right.code,
                    &[],
                ),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_order_precedes_the_normative_tie_breaker() {
        let selected = select_parse_defect([
            ParseDefectCandidate {
                encounter_order: 2,
                span: Span::new(0, 1),
                code: "a",
                payload: 2,
            },
            ParseDefectCandidate {
                encounter_order: 1,
                span: Span::new(8, 9),
                code: "z",
                payload: 1,
            },
        ])
        .expect("candidate");
        assert_eq!(selected.payload, 1);
    }

    #[test]
    fn equal_discovery_order_uses_span_then_code() {
        let selected = select_parse_defect([
            ParseDefectCandidate {
                encounter_order: 1,
                span: Span::new(8, 9),
                code: "a",
                payload: 2,
            },
            ParseDefectCandidate {
                encounter_order: 1,
                span: Span::new(2, 3),
                code: "z",
                payload: 1,
            },
        ])
        .expect("candidate");
        assert_eq!(selected.payload, 1);
    }
}
