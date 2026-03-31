//! CSS and class flow facts.
//!
//! Two distinct result families:
//! - `ClassFlowFact` — scoped/module/root/fallthrough-aware class forwarding
//! - `CssBleedIssue` — global/non-scoped selector collision reporting

use serde::{Deserialize, Serialize};
use verter_span::Span;

/// Style scope kind for a style block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StyleScopeKind {
    Scoped,
    Module,
    Global,
    /// `:global()` escape within a scoped block.
    GlobalEscape,
}

/// Certainty of a class forwarding path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClassFlowCertainty {
    /// The class is definitely forwarded along this path.
    Definite,
    /// The class may be forwarded (conditional branches, dynamic classes).
    Possible,
    /// Cannot determine whether the class is forwarded.
    Unknown,
}

/// A class forwarding fact through a component-instance edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassFlowFact {
    pub class_name: String,
    pub source_file_id: String,
    pub target_file_id: String,
    pub certainty: ClassFlowCertainty,
    pub span: Span,
}

/// Likelihood of a CSS bleed issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CssBleedLikelihood {
    /// The selector definitely bleeds (non-scoped, matching elements exist).
    Definite,
    /// The selector likely bleeds (common patterns, probable co-rendering).
    Likely,
    /// The selector possibly bleeds (uncertain co-rendering).
    Possible,
    /// Cannot determine bleed likelihood.
    Unknown,
}

/// A global CSS bleed issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CssBleedIssue {
    pub selector: String,
    pub source_file_id: String,
    pub likelihood: CssBleedLikelihood,
    pub scope_kind: StyleScopeKind,
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_flow_certainty_variants() {
        assert_ne!(ClassFlowCertainty::Definite, ClassFlowCertainty::Possible);
        assert_ne!(ClassFlowCertainty::Possible, ClassFlowCertainty::Unknown);
    }

    #[test]
    fn css_bleed_severity_mapping() {
        // Plan: Definite bleed + Definite/Possible co-render → warning
        //       Likely bleed + Possible co-render → hint
        //       Possible bleed or Unknown → trace only
        let definite = CssBleedLikelihood::Definite;
        let likely = CssBleedLikelihood::Likely;
        let possible = CssBleedLikelihood::Possible;
        assert_ne!(definite, likely);
        assert_ne!(likely, possible);
    }

    #[test]
    fn style_scope_kind_round_trips() {
        for kind in [
            StyleScopeKind::Scoped,
            StyleScopeKind::Module,
            StyleScopeKind::Global,
            StyleScopeKind::GlobalEscape,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: StyleScopeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }
}
