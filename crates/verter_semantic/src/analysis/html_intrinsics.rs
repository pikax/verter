//! Vue-facing native HTML intrinsic attribute and event catalog.
//!
//! The raw catalog is generated from `@vue/runtime-dom/dist/runtime-dom.d.ts`
//! by `scripts/generate-html-intrinsics.js`. This wrapper keeps the public
//! Rust API stable while the generated file owns the source-of-truth member
//! list and tag mapping. Hosts may also materialize project-local intrinsic
//! surfaces from the consumer project's installed TypeScript/Vue JSX
//! entrypoints.
//!
//! Member TYPE shapes live in the deterministic static catalog
//! ([`html_intrinsic_catalog`]): a member carries only a content-free
//! [`StaticIntrinsicTypeId`] into the id ↔ shape table, never an embedded type
//! body. The table is built ONCE from the generated member tables in generated
//! order (dedup interning), so ids are deterministic and reproducible across
//! processes; hosts lower ids/shapes into graph handles on demand.

use std::sync::OnceLock;

use verter_type_expr::intrinsics::{
    IntrinsicMemberFact, StaticIntrinsicTable, StaticIntrinsicTypeId,
};
use verter_type_expr::PrimitiveName;

pub use verter_type_expr::intrinsics::IntrinsicMemberKind;

/// A single intrinsic member (attr or listener) for an HTML element. The
/// member's type SHAPE is recovered from [`html_intrinsic_catalog`] by
/// `type_id`, never stored on the member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrinsicMember {
    pub name: &'static str,
    pub kind: IntrinsicMemberKind,
    pub type_id: StaticIntrinsicTypeId,
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

/// The static type SHAPE of one intrinsic catalog entry — table-resident data
/// recovered from a [`StaticIntrinsicTypeId`], never carried on a member fact.
/// Display text is preserved verbatim from the generated table so a host can
/// lower it into its own type representation on demand.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntrinsicTypeShape {
    /// A primitive attr type (`string` / `number` / `boolean`).
    Primitive(PrimitiveName),
    /// A non-primitive attr type — the generated display text, verbatim.
    AttrDisplay(String),
    /// A listener function type — the generated display text normalized to a
    /// function form (a bare event payload type renders as
    /// `(payload: T) => void`).
    ListenerFunction(String),
}

/// The deterministic static HTML intrinsic catalog: the id ↔ shape interner
/// built ONCE from every generated member table in generated order (dedup by
/// shape), so an id is stable for the life of the catalog data and identical
/// across processes running the same build.
pub struct HtmlIntrinsicCatalog {
    table: StaticIntrinsicTable<IntrinsicTypeShape>,
}

impl HtmlIntrinsicCatalog {
    /// Build the catalog from the generated member tables in generated order.
    fn build() -> Self {
        let mut table = StaticIntrinsicTable::new();
        for members in ALL_MEMBER_TABLES {
            for raw in *members {
                table.intern(raw_type_shape(raw.kind, raw.raw_type));
            }
        }
        Self { table }
    }

    /// The shape for a catalog id (`None` for a fabricated / out-of-range id).
    #[must_use]
    pub fn shape(&self, id: StaticIntrinsicTypeId) -> Option<&IntrinsicTypeShape> {
        self.table.shape(id)
    }

    /// The interned id for an EQUAL shape (`None` when no generated member
    /// produced that shape).
    #[must_use]
    pub fn id_for(&self, shape: &IntrinsicTypeShape) -> Option<StaticIntrinsicTypeId> {
        self.table.id_for(shape)
    }

    /// Number of distinct interned shapes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// Whether the catalog is empty (never true for the generated data).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

/// The process-global deterministic intrinsic catalog.
pub fn html_intrinsic_catalog() -> &'static HtmlIntrinsicCatalog {
    static CATALOG: OnceLock<HtmlIntrinsicCatalog> = OnceLock::new();
    CATALOG.get_or_init(HtmlIntrinsicCatalog::build)
}

/// The catalog shape of one generated raw member. Primitive attr types fold to
/// [`IntrinsicTypeShape::Primitive`]; every other generated display text is
/// preserved verbatim in the table (listener payload types normalize to a
/// function form). This mirrors the generated-data boundary exactly — the raw
/// type IS display text from the generated table, so text inspection here is
/// the producer boundary, not a resolver heuristic.
fn raw_type_shape(kind: RawIntrinsicMemberKind, raw_type: &str) -> IntrinsicTypeShape {
    match kind {
        RawIntrinsicMemberKind::Attr => match raw_type {
            "string" => IntrinsicTypeShape::Primitive(PrimitiveName::String),
            "number" => IntrinsicTypeShape::Primitive(PrimitiveName::Number),
            "boolean" => IntrinsicTypeShape::Primitive(PrimitiveName::Boolean),
            other => IntrinsicTypeShape::AttrDisplay(other.to_string()),
        },
        RawIntrinsicMemberKind::Listener => {
            let display = if raw_type.contains("=>") {
                raw_type.to_string()
            } else {
                format!("(payload: {raw_type}) => void")
            };
            IntrinsicTypeShape::ListenerFunction(display)
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

    let type_id = html_intrinsic_catalog()
        .id_for(&raw_type_shape(raw.kind, raw.raw_type))
        .expect("every generated member's shape is interned at catalog build");
    IntrinsicMember {
        name: raw.name,
        kind,
        type_id,
    }
}

/// Convert the built-in fallback catalog into owned member FACTS for host-side
/// use (the fact carries the content-free catalog id; the shape is recovered
/// from [`html_intrinsic_catalog`]).
pub fn owned_intrinsic_members_for_tag(tag: &str) -> Vec<IntrinsicMemberFact> {
    intrinsic_members_for_tag(tag)
        .into_iter()
        .map(|member| IntrinsicMemberFact {
            name: member.name.to_string(),
            kind: member.kind,
            type_id: member.type_id,
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
