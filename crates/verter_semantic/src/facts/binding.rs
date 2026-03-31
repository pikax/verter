//! Binding declaration and usage facts.
//!
//! Tracks binding declarations, their scope, and usage sites across
//! script, template, and style blocks.

use serde::{Deserialize, Serialize};
use verter_span::Span;

/// A binding declaration in the semantic model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingDeclaration {
    pub name: String,
    pub kind: BindingKind,
    pub span: Span,
    /// Where this binding is used.
    pub usages: Vec<BindingUsage>,
}

/// What kind of declaration created the binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BindingKind {
    /// `const x = ...`
    Const,
    /// `let x = ...`
    Let,
    /// `var x = ...`
    Var,
    /// `function x() { ... }`
    Function,
    /// `async function x() { ... }`
    AsyncFunction,
    /// `class X { ... }`
    Class,
    /// `import { x } from "..."` (value import)
    Import,
    /// `import type { x } from "..."` (type-only import)
    TypeImport,
    /// From `defineProps` destructuring.
    PropDestructure,
    /// From `defineEmits` return value.
    EmitReturn,
}

/// How a binding is used at a particular site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingUsage {
    pub kind: UsageKind,
    pub span: Span,
    /// Which block this usage appears in.
    pub block: UsageBlock,
}

/// The kind of usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsageKind {
    /// Simple read: `console.log(x)`
    Read,
    /// Assignment: `x = 5`
    Write,
    /// Function call: `x()`
    Call,
    /// Member access: `x.foo`
    MemberAccess,
    /// Destructuring: `const { a } = x`
    Destructure,
    /// Spread: `...x`
    Spread,
    /// Template interpolation: `{{ x }}`
    TemplateInterpolation,
    /// Template directive value: `v-if="x"`
    TemplateDirective,
    /// Style v-bind: `v-bind(x)`
    StyleVBind,
}

/// Which SFC block a usage appears in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsageBlock {
    Script,
    Template,
    Style,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_declaration_tracks_usages() {
        let decl = BindingDeclaration {
            name: "count".into(),
            kind: BindingKind::Const,
            span: Span::new(10, 15),
            usages: vec![
                BindingUsage {
                    kind: UsageKind::Read,
                    span: Span::new(100, 105),
                    block: UsageBlock::Template,
                },
                BindingUsage {
                    kind: UsageKind::Write,
                    span: Span::new(200, 205),
                    block: UsageBlock::Script,
                },
            ],
        };

        assert_eq!(decl.usages.len(), 2);
        assert_eq!(decl.usages[0].kind, UsageKind::Read);
        assert_eq!(decl.usages[0].block, UsageBlock::Template);
        assert_eq!(decl.usages[1].kind, UsageKind::Write);
        assert_eq!(decl.usages[1].block, UsageBlock::Script);
    }

    #[test]
    fn binding_kinds_are_distinct() {
        assert_ne!(BindingKind::Const, BindingKind::Let);
        assert_ne!(BindingKind::Import, BindingKind::TypeImport);
        assert_ne!(BindingKind::Function, BindingKind::AsyncFunction);
    }

    #[test]
    fn binding_declaration_serializes() {
        let decl = BindingDeclaration {
            name: "msg".into(),
            kind: BindingKind::Const,
            span: Span::new(0, 3),
            usages: vec![],
        };
        let json = serde_json::to_string(&decl).unwrap();
        assert!(json.contains("msg"));
        let back: BindingDeclaration = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "msg");
        assert_eq!(back.kind, BindingKind::Const);
    }

    #[test]
    fn all_usage_kinds_distinct() {
        let kinds = [
            UsageKind::Read,
            UsageKind::Write,
            UsageKind::Call,
            UsageKind::MemberAccess,
            UsageKind::Destructure,
            UsageKind::Spread,
            UsageKind::TemplateInterpolation,
            UsageKind::TemplateDirective,
            UsageKind::StyleVBind,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn all_usage_blocks_distinct() {
        assert_ne!(UsageBlock::Script, UsageBlock::Template);
        assert_ne!(UsageBlock::Template, UsageBlock::Style);
        assert_ne!(UsageBlock::Script, UsageBlock::Style);
    }

    #[test]
    fn binding_with_no_usages() {
        let decl = BindingDeclaration {
            name: "unused".into(),
            kind: BindingKind::Let,
            span: Span::new(0, 6),
            usages: vec![],
        };
        assert!(decl.usages.is_empty());
        assert_eq!(decl.kind, BindingKind::Let);
    }

    #[test]
    fn usage_serializes() {
        let usage = BindingUsage {
            kind: UsageKind::TemplateInterpolation,
            span: Span::new(100, 105),
            block: UsageBlock::Template,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: BindingUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, UsageKind::TemplateInterpolation);
        assert_eq!(back.block, UsageBlock::Template);
    }

    #[test]
    fn prop_destructure_kind() {
        assert_ne!(BindingKind::PropDestructure, BindingKind::Const);
        assert_ne!(BindingKind::EmitReturn, BindingKind::Function);
    }
}
