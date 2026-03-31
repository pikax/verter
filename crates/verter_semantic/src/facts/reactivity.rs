//! Reactivity provenance and classification facts.
//!
//! Tracks how values acquire or lose reactivity through the data flow graph.
//! Supports queries like "why is this binding reactive?" and "where does
//! reactivity get lost?"

use serde::{Deserialize, Serialize};
use verter_span::Span;

/// Overall reactivity conclusion for a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReactivityStatus {
    /// Confirmed reactive — holds a reactive value (ref, reactive, computed, etc.).
    Reactive,
    /// Confirmed non-reactive — plain value that never changes reactively.
    NonReactive,
    /// May or may not be reactive — composable return, conditional, etc.
    MaybeReactive,
    /// Insufficient information to determine reactivity.
    Unknown,
}

/// Where a value's reactivity originates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReactivitySource {
    /// `ref()`, `shallowRef()`, `customRef()`
    Ref,
    /// `reactive()`, `shallowReactive()`
    Reactive,
    /// `computed()`
    Computed,
    /// `readonly()`, `shallowReadonly()`
    Readonly,
    /// Component props (inherently reactive).
    Props,
    /// `inject()` — reactive if provider value is reactive.
    Inject,
    /// Pinia/Vuex store state.
    Store,
    /// Composable return value.
    Composable,
    /// `toRef()`, `toRefs()`
    ToRef,
    /// `storeToRefs()`
    StoreToRefs,
}

/// A step in a reactivity provenance trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceStep {
    pub kind: ProvenanceStepKind,
    pub span: Span,
    pub description: String,
}

/// The kind of transition in a provenance trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProvenanceStepKind {
    /// Origin: where the reactive value was created.
    Source,
    /// Alias: `const y = x` (preserves reactivity).
    Alias,
    /// Projection: `x.value`, `x.foo` (may preserve or lose reactivity).
    Projection,
    /// Destructuring: `const { a } = x` (loses reactivity without toRefs).
    Destructure,
    /// Escape: value passed out of reactive scope.
    Escape,
    /// Loss: reactivity definitively lost at this point.
    Loss,
    /// Effect read: used as dependency in computed/watch/template.
    EffectRead,
    /// Import: crosses a module boundary.
    Import,
    /// Re-export: crosses a module boundary as re-export.
    ReExport,
}

/// Full reactivity analysis result for a binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactivityFact {
    pub status: ReactivityStatus,
    pub source: Option<ReactivitySource>,
    pub trace: Vec<ProvenanceStep>,
}

impl ReactivityFact {
    pub fn non_reactive() -> Self {
        Self {
            status: ReactivityStatus::NonReactive,
            source: None,
            trace: Vec::new(),
        }
    }

    pub fn unknown() -> Self {
        Self {
            status: ReactivityStatus::Unknown,
            source: None,
            trace: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactivity_status_variants_are_distinct() {
        let statuses = [
            ReactivityStatus::Reactive,
            ReactivityStatus::NonReactive,
            ReactivityStatus::MaybeReactive,
            ReactivityStatus::Unknown,
        ];
        for (i, a) in statuses.iter().enumerate() {
            for (j, b) in statuses.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn non_reactive_fact_has_no_source() {
        let fact = ReactivityFact::non_reactive();
        assert_eq!(fact.status, ReactivityStatus::NonReactive);
        assert!(fact.source.is_none());
        assert!(fact.trace.is_empty());
    }

    #[test]
    fn reactive_fact_with_trace() {
        let fact = ReactivityFact {
            status: ReactivityStatus::Reactive,
            source: Some(ReactivitySource::Ref),
            trace: vec![
                ProvenanceStep {
                    kind: ProvenanceStepKind::Source,
                    span: Span::new(10, 20),
                    description: "ref(0)".into(),
                },
                ProvenanceStep {
                    kind: ProvenanceStepKind::Alias,
                    span: Span::new(30, 40),
                    description: "const count = myRef".into(),
                },
            ],
        };

        assert_eq!(fact.status, ReactivityStatus::Reactive);
        assert_eq!(fact.source, Some(ReactivitySource::Ref));
        assert_eq!(fact.trace.len(), 2);
        assert_eq!(fact.trace[0].kind, ProvenanceStepKind::Source);
        assert_eq!(fact.trace[1].kind, ProvenanceStepKind::Alias);
    }

    #[test]
    fn destructure_loss_without_to_refs() {
        let fact = ReactivityFact {
            status: ReactivityStatus::NonReactive,
            source: Some(ReactivitySource::Props),
            trace: vec![
                ProvenanceStep {
                    kind: ProvenanceStepKind::Source,
                    span: Span::new(5, 10),
                    description: "defineProps".into(),
                },
                ProvenanceStep {
                    kind: ProvenanceStepKind::Destructure,
                    span: Span::new(15, 30),
                    description: "const { msg } = props".into(),
                },
                ProvenanceStep {
                    kind: ProvenanceStepKind::Loss,
                    span: Span::new(15, 30),
                    description: "destructuring loses reactivity without toRefs".into(),
                },
            ],
        };

        // Positive: trace explains the loss
        assert_eq!(fact.trace.len(), 3);
        assert_eq!(fact.trace[2].kind, ProvenanceStepKind::Loss);

        // Negative: result is NonReactive despite Props source
        assert_eq!(fact.status, ReactivityStatus::NonReactive);
    }

    #[test]
    fn all_reactivity_sources_distinct() {
        let sources = [
            ReactivitySource::Ref,
            ReactivitySource::Reactive,
            ReactivitySource::Computed,
            ReactivitySource::Readonly,
            ReactivitySource::Props,
            ReactivitySource::Inject,
            ReactivitySource::Store,
            ReactivitySource::Composable,
            ReactivitySource::ToRef,
            ReactivitySource::StoreToRefs,
        ];
        for (i, a) in sources.iter().enumerate() {
            for (j, b) in sources.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn all_provenance_step_kinds_distinct() {
        let kinds = [
            ProvenanceStepKind::Source,
            ProvenanceStepKind::Alias,
            ProvenanceStepKind::Projection,
            ProvenanceStepKind::Destructure,
            ProvenanceStepKind::Escape,
            ProvenanceStepKind::Loss,
            ProvenanceStepKind::EffectRead,
            ProvenanceStepKind::Import,
            ProvenanceStepKind::ReExport,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn reactivity_fact_serializes() {
        let fact = ReactivityFact {
            status: ReactivityStatus::Reactive,
            source: Some(ReactivitySource::Computed),
            trace: vec![ProvenanceStep {
                kind: ProvenanceStepKind::Source,
                span: Span::new(10, 40),
                description: "computed(() => x.value * 2)".into(),
            }],
        };
        let json = serde_json::to_string(&fact).unwrap();
        let back: ReactivityFact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, ReactivityStatus::Reactive);
        assert_eq!(back.source, Some(ReactivitySource::Computed));
        assert_eq!(back.trace.len(), 1);
    }

    #[test]
    fn unknown_fact() {
        let fact = ReactivityFact::unknown();
        assert_eq!(fact.status, ReactivityStatus::Unknown);
        assert!(fact.source.is_none());
        assert!(fact.trace.is_empty());
    }
}
