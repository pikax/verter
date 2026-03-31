//! Stable external references for cross-boundary serialization.
//!
//! Internal interned IDs (SymbolId, BindingId, etc.) do not cross the
//! session/protocol boundary. Public APIs use these stable refs which
//! include canonical file IDs, stable keys, and SFC-absolute spans.
//!
//! Name-based lookups are allowed only for discovery APIs. Follow-up
//! semantic queries must consume stable refs.

use serde::{Deserialize, Serialize};
use verter_span::Span;

/// Reference to a file by its canonical ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileRef {
    pub file_id: String,
}

/// Reference to a binding declaration.
///
/// The `binding_key` is an unambiguous local discriminator (not a display label).
/// `name` is descriptive and must not be treated as sole identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BindingRef {
    pub file_id: String,
    pub binding_key: String,
    pub name: String,
    pub span: Span,
}

/// Reference to an exported or re-exported symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolRef {
    pub file_id: String,
    pub symbol_key: String,
    pub display_name: String,
    pub span: Span,
}

/// Reference to a Vue component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentRef {
    pub file_id: String,
    pub component_key: String,
    pub export_name: String,
    pub span: Span,
}

/// Reference to a computed property.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComputedRef {
    pub file_id: String,
    pub computed_key: String,
    pub name: String,
    pub span: Span,
}

/// Reference to a route definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouteRef {
    pub file_id: String,
    pub route_key: String,
    pub span: Span,
}

impl FileRef {
    pub fn new(file_id: impl Into<String>) -> Self {
        Self {
            file_id: file_id.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_ref_equality() {
        let a = FileRef::new("src/App.vue");
        let b = FileRef::new("src/App.vue");
        let c = FileRef::new("src/Other.vue");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn binding_ref_uses_key_not_just_name() {
        let a = BindingRef {
            file_id: "f.vue".into(),
            binding_key: "setup_0".into(),
            name: "count".into(),
            span: Span::new(10, 15),
        };
        let b = BindingRef {
            file_id: "f.vue".into(),
            binding_key: "setup_1".into(),
            name: "count".into(),
            span: Span::new(30, 35),
        };
        // Same name but different keys — must not be equal
        assert_ne!(a, b);
    }

    #[test]
    fn component_ref_serializes() {
        let r = ComponentRef {
            file_id: "src/Button.vue".into(),
            component_key: "default".into(),
            export_name: "default".into(),
            span: Span::new(0, 100),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("Button.vue"));
        let back: ComponentRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
