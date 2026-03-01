//! `DiagnosticSet` — a container for diagnostics with enrichment support.
//!
//! Diagnostics can be added by rules, then queried and enhanced by external
//! enrichment sources (e.g., TSGO type checker) before consumption.

use crate::diagnostic::LintDiagnostic;

/// A collection of diagnostics with querying and enrichment capabilities.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticSet {
    diagnostics: Vec<LintDiagnostic>,
}

impl DiagnosticSet {
    /// Create an empty diagnostic set.
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Create a diagnostic set from an existing vector.
    pub fn from_vec(diagnostics: Vec<LintDiagnostic>) -> Self {
        Self { diagnostics }
    }

    // ── Adding ──────────────────────────────────────────────────────────

    /// Add a single diagnostic.
    pub fn add(&mut self, diag: LintDiagnostic) {
        self.diagnostics.push(diag);
    }

    /// Extend with diagnostics from another set.
    pub fn extend(&mut self, other: DiagnosticSet) {
        self.diagnostics.extend(other.diagnostics);
    }

    // ── Querying (for enrichment) ───────────────────────────────────────

    /// Find diagnostics by rule name, returning `(index, &diag)` pairs.
    pub fn find_by_rule<'a>(
        &'a self,
        rule: &'a str,
    ) -> impl Iterator<Item = (usize, &'a LintDiagnostic)> {
        self.diagnostics
            .iter()
            .enumerate()
            .filter(move |(_, d)| d.rule == rule)
    }

    /// Find diagnostics overlapping a byte span range.
    pub fn find_by_span(
        &self,
        start: u32,
        end: u32,
    ) -> impl Iterator<Item = (usize, &LintDiagnostic)> {
        self.diagnostics
            .iter()
            .enumerate()
            .filter(move |(_, d)| d.span.start < end && d.span.end > start)
    }

    // ── Enhancing ───────────────────────────────────────────────────────

    /// Mutate a diagnostic at `index` via a closure.
    pub fn enhance(&mut self, index: usize, f: impl FnOnce(&mut LintDiagnostic)) {
        if let Some(diag) = self.diagnostics.get_mut(index) {
            f(diag);
        }
    }

    // ── Consuming ───────────────────────────────────────────────────────

    /// Consume the set and return the underlying vector.
    pub fn into_diagnostics(self) -> Vec<LintDiagnostic> {
        self.diagnostics
    }

    /// Iterate over diagnostics.
    pub fn iter(&self) -> impl Iterator<Item = &LintDiagnostic> {
        self.diagnostics.iter()
    }

    /// Number of diagnostics in the set.
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Whether the set contains no diagnostics.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};

    fn make_diag(rule: &str, start: u32, end: u32) -> LintDiagnostic {
        LintDiagnostic {
            rule: rule.to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: format!("msg for {}", rule),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ElementOpenTag,
        }
    }

    #[test]
    fn new_set_is_empty() {
        let set = DiagnosticSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn add_and_consume() {
        let mut set = DiagnosticSet::new();
        set.add(make_diag("rule-a", 0, 10));
        set.add(make_diag("rule-b", 20, 30));
        assert_eq!(set.len(), 2);

        let diags = set.into_diagnostics();
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].rule, "rule-a");
        assert_eq!(diags[1].rule, "rule-b");
    }

    #[test]
    fn find_by_rule() {
        let mut set = DiagnosticSet::new();
        set.add(make_diag("rule-a", 0, 10));
        set.add(make_diag("rule-b", 20, 30));
        set.add(make_diag("rule-a", 40, 50));

        let found: Vec<_> = set.find_by_rule("rule-a").collect();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, 0); // index 0
        assert_eq!(found[1].0, 2); // index 2

        let not_found: Vec<_> = set.find_by_rule("nonexistent").collect();
        assert!(
            not_found.is_empty(),
            "nonexistent rule should match nothing"
        );
    }

    #[test]
    fn find_by_span_overlap() {
        let mut set = DiagnosticSet::new();
        set.add(make_diag("a", 0, 10)); // [0, 10)
        set.add(make_diag("b", 20, 30)); // [20, 30)
        set.add(make_diag("c", 5, 25)); // [5, 25)

        // Query [8, 22) overlaps a=[0,10), b=[20,30), c=[5,25)
        let found: Vec<_> = set.find_by_span(8, 22).collect();
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].1.rule, "a");
        assert_eq!(found[1].1.rule, "b");
        assert_eq!(found[2].1.rule, "c");

        // Query [11, 19) should overlap only c=[5,25)
        let mid: Vec<_> = set.find_by_span(11, 19).collect();
        assert_eq!(mid.len(), 1);
        assert_eq!(mid[0].1.rule, "c");

        // Query [31, 40) should overlap nothing
        let empty: Vec<_> = set.find_by_span(31, 40).collect();
        assert!(empty.is_empty(), "no overlap expected");
    }

    #[test]
    fn enhance_modifies_diagnostic() {
        let mut set = DiagnosticSet::new();
        set.add(make_diag("rule-a", 0, 10));
        set.enhance(0, |d| {
            d.tags.push(DiagnosticTag::Unnecessary);
            d.message = "enhanced".to_string();
        });

        let diags = set.into_diagnostics();
        assert_eq!(diags[0].message, "enhanced");
        assert_eq!(diags[0].tags, vec![DiagnosticTag::Unnecessary]);
    }

    #[test]
    fn enhance_out_of_bounds_is_noop() {
        let mut set = DiagnosticSet::new();
        set.add(make_diag("rule-a", 0, 10));
        // Should not panic
        set.enhance(99, |d| d.message = "should not happen".to_string());
        assert_eq!(set.into_diagnostics()[0].message, "msg for rule-a");
    }

    #[test]
    fn extend_merges_sets() {
        let mut a = DiagnosticSet::new();
        a.add(make_diag("a", 0, 10));

        let mut b = DiagnosticSet::new();
        b.add(make_diag("b", 20, 30));

        a.extend(b);
        assert_eq!(a.len(), 2);
        let diags = a.into_diagnostics();
        assert_eq!(diags[0].rule, "a");
        assert_eq!(diags[1].rule, "b");
    }

    #[test]
    fn from_vec_preserves_diagnostics() {
        let diags = vec![make_diag("x", 0, 5), make_diag("y", 10, 15)];
        let set = DiagnosticSet::from_vec(diags);
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
    }
}
