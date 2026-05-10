//! Vue-facing native HTML intrinsic attribute and event catalog.
//!
//! The raw catalog is generated from `@vue/runtime-dom/dist/runtime-dom.d.ts`
//! by `scripts/generate-html-intrinsics.js`. This wrapper keeps the public
//! Rust API stable while the generated file owns the source-of-truth member
//! list and tag mapping. Hosts may also materialize project-local intrinsic
//! surfaces from the consumer project's installed TypeScript/Vue JSX
//! entrypoints.

use verter_type_expr::{PrimitiveName, TypeExpr};

/// Kind of intrinsic member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicMemberKind {
    /// HTML attribute (e.g., `id`, `disabled`, `placeholder`).
    Attr,
    /// Event listener (e.g., `click`, `focus`). Name is the event name, not `onXxx`.
    Listener,
}

/// A single intrinsic member (attr or listener) for an HTML element.
#[derive(Debug, Clone)]
pub struct IntrinsicMember {
    pub name: &'static str,
    pub kind: IntrinsicMemberKind,
    pub type_expr: TypeExpr,
}

/// Owned intrinsic member used by host/runtime intrinsic surfaces.
#[derive(Debug, Clone)]
pub struct OwnedIntrinsicMember {
    pub name: String,
    pub kind: IntrinsicMemberKind,
    pub type_expr: TypeExpr,
}

#[derive(Clone, Copy)]
pub(crate) struct RawIntrinsicMember {
    pub(crate) name: &'static str,
    pub(crate) kind: RawIntrinsicMemberKind,
    pub(crate) raw_type: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawIntrinsicMemberKind {
    Attr,
    Listener,
}

include!("html_intrinsics_data.rs");

fn raw_type_to_type_expr(kind: RawIntrinsicMemberKind, raw_type: &str) -> TypeExpr {
    match kind {
        RawIntrinsicMemberKind::Attr => match raw_type {
            "string" => TypeExpr::Primitive(PrimitiveName::String),
            "number" => TypeExpr::Primitive(PrimitiveName::Number),
            "boolean" => TypeExpr::Primitive(PrimitiveName::Boolean),
            other => TypeExpr::Unknown {
                raw: other.to_string(),
            },
        },
        RawIntrinsicMemberKind::Listener => {
            let raw = if raw_type.contains("=>") {
                raw_type.to_string()
            } else {
                format!("(payload: {raw_type}) => void")
            };
            TypeExpr::Unknown { raw }
        }
    }
}

pub fn should_expose_intrinsic_member(kind: IntrinsicMemberKind, name: &str) -> bool {
    if kind != IntrinsicMemberKind::Attr {
        return true;
    }

    !matches!(
        name,
        "innerHTML" | "innerText" | "key" | "ref" | "ref_for" | "ref_key" | "textContent"
    )
}

fn convert_member(raw: &RawIntrinsicMember) -> IntrinsicMember {
    let kind = match raw.kind {
        RawIntrinsicMemberKind::Attr => IntrinsicMemberKind::Attr,
        RawIntrinsicMemberKind::Listener => IntrinsicMemberKind::Listener,
    };

    IntrinsicMember {
        name: raw.name,
        kind,
        type_expr: raw_type_to_type_expr(raw.kind, raw.raw_type),
    }
}

/// Convert the built-in fallback catalog into owned members for host-side use.
pub fn owned_intrinsic_members_for_tag(tag: &str) -> Vec<OwnedIntrinsicMember> {
    intrinsic_members_for_tag(tag)
        .into_iter()
        .map(|member| OwnedIntrinsicMember {
            name: member.name.to_string(),
            kind: member.kind,
            type_expr: member.type_expr,
        })
        .collect()
}

/// Get all Vue-facing intrinsic members (attrs + listeners) for an HTML tag.
///
/// Returns the full Vue-facing public surface from `@vue/runtime-dom`.
/// Unknown/custom tags fall back to the generic `HTMLAttributes` surface.
pub fn intrinsic_members_for_tag(tag: &str) -> Vec<IntrinsicMember> {
    raw_members_for_tag(tag)
        .iter()
        .map(convert_member)
        .filter(|member| should_expose_intrinsic_member(member.kind, member.name))
        .collect()
}

/// Get only the attr members (non-listener) for a tag.
pub fn intrinsic_attrs_for_tag(tag: &str) -> Vec<IntrinsicMember> {
    intrinsic_members_for_tag(tag)
        .into_iter()
        .filter(|member| member.kind == IntrinsicMemberKind::Attr)
        .collect()
}

/// Get only the listener members for a tag.
pub fn intrinsic_listeners_for_tag(tag: &str) -> Vec<IntrinsicMember> {
    intrinsic_members_for_tag(tag)
        .into_iter()
        .filter(|member| member.kind == IntrinsicMemberKind::Listener)
        .collect()
}

/// Convert a Vue listener prop name like `onClick` into canonical event-name
/// form like `click`.
pub fn on_prop_to_event_name(on_prop: &str) -> Option<String> {
    if on_prop.len() > 2 && on_prop.starts_with("on") && on_prop.as_bytes()[2].is_ascii_uppercase()
    {
        let event_name = on_prop[2..3].to_lowercase() + &on_prop[3..];
        Some(event_name)
    } else {
        None
    }
}

/// Convert a canonical event name like `click` into Vue listener prop form
/// like `onClick`.
pub fn event_name_to_on_prop(event_name: &str) -> String {
    if event_name.is_empty() {
        return "on".to_string();
    }
    let mut chars = event_name.chars();
    let first = chars.next().unwrap();
    format!("on{}{}", first.to_ascii_uppercase(), chars.as_str())
}

#[cfg(test)]
#[path = "html_intrinsics_tests.rs"]
mod html_intrinsics_tests;
