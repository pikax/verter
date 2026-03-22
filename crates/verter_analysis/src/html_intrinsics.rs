//! Vue-facing native HTML intrinsic attribute and event catalog.
//!
//! The raw catalog is generated from `@vue/runtime-dom/dist/runtime-dom.d.ts`
//! by `scripts/generate-html-intrinsics.js`. This wrapper keeps the public
//! Rust API stable while the generated file owns the source-of-truth member
//! list and tag mapping. At runtime, hosts may also inject a project-local
//! intrinsic catalog extracted from the consumer project's installed
//! TypeScript/Vue JSX surface.

use crate::type_expr::{PrimitiveName, TypeExpr};
use rustc_hash::FxHashMap;

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

/// Owned intrinsic member used by project-local override catalogs.
#[derive(Debug, Clone)]
pub struct OwnedIntrinsicMember {
    pub name: String,
    pub kind: IntrinsicMemberKind,
    pub type_expr: TypeExpr,
}

/// Runtime project-local intrinsic catalog derived from installed TS/Vue types.
#[derive(Debug, Clone, Default)]
pub struct ProjectHtmlIntrinsicCatalog {
    fallback_members: Vec<OwnedIntrinsicMember>,
    tag_members: FxHashMap<String, Vec<OwnedIntrinsicMember>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectHtmlIntrinsicCatalogWire {
    #[serde(default)]
    fallback: Vec<ProjectHtmlIntrinsicMemberWire>,
    #[serde(default)]
    tags: Vec<ProjectHtmlIntrinsicTagWire>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectHtmlIntrinsicTagWire {
    tag: String,
    #[serde(default)]
    members: Vec<ProjectHtmlIntrinsicMemberWire>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectHtmlIntrinsicMemberWire {
    name: String,
    kind: ProjectHtmlIntrinsicMemberKindWire,
    raw_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum ProjectHtmlIntrinsicMemberKindWire {
    Attr,
    Listener,
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

fn should_expose_intrinsic_member(kind: IntrinsicMemberKind, name: &str) -> bool {
    if kind != IntrinsicMemberKind::Attr {
        return true;
    }

    !matches!(
        name,
        "innerHTML" | "innerText" | "key" | "ref" | "ref_for" | "ref_key" | "textContent"
    )
}

/// Convert a raw project-local intrinsic type string into Verter's shared type IR.
pub fn raw_intrinsic_type_to_type_expr(kind: IntrinsicMemberKind, raw_type: &str) -> TypeExpr {
    raw_type_to_type_expr(
        match kind {
            IntrinsicMemberKind::Attr => RawIntrinsicMemberKind::Attr,
            IntrinsicMemberKind::Listener => RawIntrinsicMemberKind::Listener,
        },
        raw_type,
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

fn convert_owned_member(wire: ProjectHtmlIntrinsicMemberWire) -> OwnedIntrinsicMember {
    let kind = match wire.kind {
        ProjectHtmlIntrinsicMemberKindWire::Attr => IntrinsicMemberKind::Attr,
        ProjectHtmlIntrinsicMemberKindWire::Listener => IntrinsicMemberKind::Listener,
    };

    OwnedIntrinsicMember {
        name: wire.name,
        kind,
        type_expr: raw_intrinsic_type_to_type_expr(kind, &wire.raw_type),
    }
}

impl ProjectHtmlIntrinsicCatalog {
    /// Parse a project-local intrinsic override catalog encoded as JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let wire: ProjectHtmlIntrinsicCatalogWire = serde_json::from_str(json)?;
        Ok(Self {
            fallback_members: wire
                .fallback
                .into_iter()
                .map(convert_owned_member)
                .filter(|member| should_expose_intrinsic_member(member.kind, &member.name))
                .collect(),
            tag_members: wire
                .tags
                .into_iter()
                .map(|tag| {
                    (
                        tag.tag,
                        tag.members
                            .into_iter()
                            .map(convert_owned_member)
                            .filter(|member| {
                                should_expose_intrinsic_member(member.kind, &member.name)
                            })
                            .collect(),
                    )
                })
                .collect(),
        })
    }

    /// Look up the project-local surface for a tag.
    ///
    /// Returns `fallback` members when present if the catalog does not define a
    /// tag-specific entry.
    pub fn members_for_tag(&self, tag: &str) -> Option<&[OwnedIntrinsicMember]> {
        self.tag_members.get(tag).map(Vec::as_slice).or_else(|| {
            (!self.fallback_members.is_empty()).then_some(self.fallback_members.as_slice())
        })
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
