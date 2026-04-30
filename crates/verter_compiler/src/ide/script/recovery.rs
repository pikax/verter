//! TSX recovery + partial-AST assessment helpers (test-only).
//!
//! OXC 0.116.0 does NOT produce partial ASTs — `program.body` is empty for
//! all error cases. These types and the `assess_partial_ast()` function are
//! kept behind `#[cfg(test)]` for the Category E assessment tests. If a
//! future OXC version starts producing partial ASTs, this code can be
//! promoted to production.

#[cfg(test)]
use oxc_span::GetSpan;

#[cfg(test)]
use crate::utils::oxc::vue::ScriptItem;

#[cfg(test)]
use super::macros::macro_span;

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PartialAstStrategy {
    Normal,
    NormalSkipDamagedMacros,
    ErrorRecovery,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug)]
pub(super) struct PartialAstAssessment {
    pub(super) clean_stmt_count: usize,
    pub(super) total_stmt_count: usize,
    pub(super) clean_import_count: usize,
    pub(super) clean_macro_count: usize,
    pub(super) damaged_macro_count: usize,
    pub(super) damaged_import_count: usize,
    pub(super) strategy: PartialAstStrategy,
}

#[cfg(test)]
#[allow(dead_code)]
pub(super) fn assess_partial_ast(
    program: &oxc_ast::ast::Program<'_>,
    errors: &[oxc_diagnostics::OxcDiagnostic],
    parse_result: &crate::utils::oxc::vue::ScriptParseResult<'_>,
) -> PartialAstAssessment {
    let total_stmt_count = program.body.len();

    if total_stmt_count == 0 {
        return PartialAstAssessment {
            clean_stmt_count: 0,
            total_stmt_count: 0,
            clean_import_count: 0,
            clean_macro_count: 0,
            damaged_macro_count: 0,
            damaged_import_count: 0,
            strategy: PartialAstStrategy::ErrorRecovery,
        };
    }

    let error_ranges: Vec<(u32, u32)> = errors
        .iter()
        .flat_map(|e| e.labels.iter().flatten())
        .map(|label| (label.offset() as u32, (label.offset() + label.len()) as u32))
        .collect();

    let clean_stmt_count = program
        .body
        .iter()
        .filter(|stmt| {
            let span = stmt.span();
            !overlaps_any(&error_ranges, span.start, span.end)
        })
        .count();

    let mut clean_macro_count = 0usize;
    let mut damaged_macro_count = 0usize;
    let mut clean_import_count = 0usize;
    let mut damaged_import_count = 0usize;

    for item in &parse_result.items {
        match item {
            ScriptItem::Macro(m) => {
                let span = macro_span(m);
                if overlaps_any(&error_ranges, span.start, span.end) {
                    damaged_macro_count += 1;
                } else {
                    clean_macro_count += 1;
                }
            }
            ScriptItem::Import(imp) => {
                if overlaps_any(&error_ranges, imp.span.start, imp.span.end) {
                    damaged_import_count += 1;
                } else {
                    clean_import_count += 1;
                }
            }
            _ => {}
        }
    }

    let strategy = if clean_stmt_count == 0 && clean_macro_count == 0 && clean_import_count == 0 {
        PartialAstStrategy::ErrorRecovery
    } else if damaged_macro_count > 0 {
        PartialAstStrategy::NormalSkipDamagedMacros
    } else {
        PartialAstStrategy::Normal
    };

    PartialAstAssessment {
        clean_stmt_count,
        total_stmt_count,
        clean_import_count,
        clean_macro_count,
        damaged_macro_count,
        damaged_import_count,
        strategy,
    }
}

/// Check if a span [start, end) overlaps with any error range.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn overlaps_any(error_ranges: &[(u32, u32)], start: u32, end: u32) -> bool {
    error_ranges
        .iter()
        .any(|&(e_start, e_end)| start < e_end && end > e_start)
}
