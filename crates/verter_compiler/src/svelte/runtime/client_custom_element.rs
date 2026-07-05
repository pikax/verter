//! Custom-element CLIENT emission — the narrow-plan payload + emitters for the
//! module epilogue (`customElements.define(tag, $.create_custom_element(…))` /
//! the bare `$.create_custom_element(…)` statement) and the component-body
//! accessor frame (`var $$exports = { get x() { … }, set x($$value) { … } };`
//! … `return $.pop($$exports)`). `$host` is rewritten to `$$props.$$host`;
//! whether the component binds a `$$props` parameter is decided by the plan
//! build's `props_param_bound` fact (a real props binder OR `needs_context` —
//! a member on the `$host()` call result is itself a `needs_context` reason,
//! in handlers and `{@render}` dynamic callees alike), NOT forced by `$host`
//! alone. The `$host`-usage facts feeding that decision are recorded by the
//! rune scan ([`super::rune_scan::UnsupportedRuneScan`]) and carried on the
//! classified surface — never re-discovered here.
//!
//! The official topology (pinned `svelte@5.56.3`):
//!
//! - `$.create_custom_element(Cmp, props, slots, accessors, shadowRootInit,
//!   extend)` — a 6-argument CONDITIONAL shape. `props` (arg2) is the resolved
//!   prop-definition object (`{}` when none); `slots` (arg3) and `accessors`
//!   (arg4) are `[]` on the supported runes surface (get/set accessors ride the
//!   body `$$exports`, never arg4); `shadowRootInit` (arg5) is `{ mode: 'open' }`
//!   for the open/default shadow, OMITTED for `shadow: 'none'` (spelled `void 0`
//!   when an `extend` arg6 follows), and the verbatim object expression for an
//!   object shadow; `extend` (arg6) is present only when given.
//! - `customElements.define(tag, …)` wraps the create call ONLY when the
//!   descriptor carries a tag; the no-tag / compile-option forms emit the bare
//!   create statement (registration is the user's).
//! - The component-body frame is FACT-DRIVEN, not blanket: the `$.push($$props,
//!   true)` / `$.pop()` context frame is driven by reactive-analysis
//!   `needs_context` (an unsafe `{@render}` callee, an unsafe member,
//!   `$effect`, …) OR non-empty custom-element accessor exports;
//!   `props_param_bound` only controls `$$props` parameter binding /
//!   bare-`$host()` admission. A real props binder without CE accessors, such
//!   as rest-only or whole-object `$props()`, does not by itself open
//!   `$.push`/`$.pop`. Only the `var $$exports = { get/set … }` accessor
//!   object (after the script statements) + `return $.pop($$exports)` is
//!   prop-accessor-gated — present iff the custom element has `$props()`
//!   members. A no-props custom element omits `$$exports` (when a
//!   `needs_context` reason holds (without accessors), its frame closes with
//!   the plain `$.pop()`).

use rustc_hash::{FxHashMap, FxHashSet};

use super::client_codegen_helpers::{js_single_quoted, object_key};
use super::expr_emit::PropsMemberPlan;
use crate::svelte::parser::{CustomElementDescriptor, CustomElementShadow};

/// The RESOLVED module-epilogue payload of a custom-element component — every
/// `create_custom_element` argument pre-rendered from the descriptor + the
/// component's `$props()` members, so the emitter only formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CustomElementEmission {
    /// The `customElements.define` tag — `None` emits the bare create statement.
    pub(super) define_tag: Option<String>,
    /// The rendered arg2 prop-definition object (`{}` /
    /// `{ count: { reflect: true, type: 'Number' } }`).
    pub(super) props_object: String,
    /// The resolved arg5 shadow axis.
    pub(super) shadow: CustomElementShadow,
    /// The verbatim arg6 `extend` expression, when given.
    pub(super) extend: Option<String>,
}

/// One `$$exports` get/set accessor pair — a custom-element prop surfaced as a
/// DOM property on the generated element class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CeExportAccessor {
    /// The accessor KEY — the prop's SOURCE key (`binding.prop_alias ?? name`:
    /// an aliased `let { count: n }` surfaces as `count`).
    pub(super) key: String,
    /// The LOCAL prop-source binding the accessor body reads/writes
    /// (`return n();` / `n($$value);`).
    pub(super) local: String,
    /// The setter parameter DEFAULT — the member default's RAW source (official
    /// prints the UNVISITED `binding.initial`, so `set count($$value = 7)`
    /// carries the raw default, never the rewritten `$.prop` thunk).
    pub(super) setter_default: Option<String>,
}

/// Build the module-epilogue payload from the resolved descriptor + the
/// component's `$props()` member plans (declaration order), mirroring the
/// official transform:
///
/// 1. Each EXPLICIT descriptor `props` entry emits under `binding.prop_alias ??
///    name` (the entry name is matched against the members' LOCAL names; an
///    aliased member surfaces its SOURCE key). A type-less entry whose matched
///    member default is a BOOLEAN literal infers `type: 'Boolean'`.
/// 2. Each member NOT covered by an explicit entry (matched by its emitted
///    source key against the RAW descriptor entry names — the official
///    `if (ce_props[key]) continue`, so an ALIASED explicit entry's member
///    still appends its inferred `{}` under the same emitted key, the official
///    duplicate-key parity) appends the inferred `key: {}` definition.
///
/// Every emitted prop-object KEY routes through [`object_key`] — the official
/// `b.key(name)` rule: an identifier-safe key stays bare, a non-identifier key
/// (`'data-id'`) becomes a quoted string-literal key (a raw non-identifier key
/// is invalid JS).
pub(super) fn build_custom_element_emission(
    descriptor: &CustomElementDescriptor,
    members: &[PropsMemberPlan],
) -> CustomElementEmission {
    // O(1) lookup indexes replacing the per-entry linear scans — behavior
    // identical: `member_by_local` keeps the FIRST member per local name
    // (`Iterator::find` parity) and `raw_descriptor_names` is the raw
    // explicit-entry name set (the official `if (ce_props[key]) continue`).
    // The EMITTED sequence stays the descriptor's + the members' SOURCE order
    // (no sort, no dedupe) — only the lookup complexity changes.
    let mut member_by_local: FxHashMap<&str, &PropsMemberPlan> = FxHashMap::default();
    for member in members {
        member_by_local
            .entry(member.local.as_str())
            .or_insert(member);
    }
    let raw_descriptor_names: FxHashSet<&str> =
        descriptor.props.iter().map(|d| d.name.as_str()).collect();
    let mut entries: Vec<String> = Vec::new();
    for def in &descriptor.props {
        // The explicit entry name is looked up as a LOCAL binding name (the
        // official `analysis.instance.scope.get(name)`); a matched ALIASED
        // member emits under its SOURCE key.
        let member = member_by_local.get(def.name.as_str()).copied();
        let key = member.map_or(def.name.as_str(), |m| m.source_key.as_str());
        // The official Boolean inference: a type-less definition whose matched
        // binding initial is a boolean LITERAL infers `type: 'Boolean'`.
        let type_hint = def.type_hint.clone().or_else(|| {
            member
                .and_then(|m| m.default.as_ref())
                .is_some_and(|facts| facts.boolean_literal)
                .then(|| "Boolean".to_string())
        });
        let mut fields = Vec::new();
        // The official transform pushes `attribute` only for a TRUTHY string
        // (`if (attribute) …`): an EMPTY `attribute: ""` OMITS the field —
        // pinned `svelte@5.56.3` emits `{ a: {} }`, never `attribute: ''`.
        if let Some(attribute) = def.attribute.as_deref().filter(|a| !a.is_empty()) {
            fields.push(format!("attribute: {}", js_single_quoted(attribute)));
        }
        if def.reflect {
            fields.push("reflect: true".to_string());
        }
        if let Some(type_hint) = &type_hint {
            fields.push(format!("type: {}", js_single_quoted(type_hint)));
        }
        entries.push(format!(
            "{}: {}",
            object_key(key),
            render_object_fields(&fields)
        ));
    }
    // The INFERRED `key: {}` remainder — every `$props()` member whose emitted
    // source key is not a RAW explicit-entry name (the official
    // `if (ce_props[key]) continue`).
    for member in members {
        if raw_descriptor_names.contains(member.source_key.as_str()) {
            continue;
        }
        entries.push(format!("{}: {{}}", object_key(&member.source_key)));
    }
    CustomElementEmission {
        define_tag: descriptor.tag.clone(),
        props_object: render_object_fields(&entries),
        shadow: descriptor.shadow.clone(),
        extend: descriptor.extend.clone(),
    }
}

/// Build the `$$exports` accessor pairs — one get/set per `$props()` member, in
/// declaration order, keyed by the SOURCE key over the LOCAL binding. The
/// setter default is the member default's RAW source slice (the official
/// unvisited `binding.initial`).
pub(super) fn build_ce_export_accessors(
    members: &[PropsMemberPlan],
    instance_source: Option<&str>,
) -> Vec<CeExportAccessor> {
    members
        .iter()
        .map(|member| CeExportAccessor {
            key: member.source_key.clone(),
            local: member.local.clone(),
            setter_default: member.default.as_ref().and_then(|facts| {
                instance_source.map(|instance| {
                    instance[facts.span.0 as usize..facts.span.1 as usize].to_string()
                })
            }),
        })
        .collect()
}

/// Render `{ a, b }` / `{}` from pre-rendered field strings.
fn render_object_fields(fields: &[String]) -> String {
    if fields.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", fields.join(", "))
    }
}

/// Emit the body `var $$exports = { get x() { … }, set x($$value[ = default]) {
/// … } };` declaration (ONE line; the golden comparison is
/// whitespace-collapsed, so the official multi-line layout and this single line
/// normalize identically). Accessor NAMES route through [`object_key`] — the
/// official `b.get(key, …)` / `b.set(key, …)` builders run `b.key(name)`, so a
/// non-identifier source key emits the quoted string-literal accessor form
/// (`get 'data-id'()` / `set 'data-id'($$value)`); a raw non-identifier
/// accessor name is invalid JS.
pub(super) fn emit_exports_object(out: &mut String, accessors: &[CeExportAccessor]) {
    let entries = accessors
        .iter()
        .map(|a| {
            let param = match &a.setter_default {
                Some(default) => format!("$$value = {default}"),
                None => "$$value".to_string(),
            };
            format!(
                "get {key}() {{ return {local}(); }}, set {key}({param}) {{ {local}($$value); $.flush(); }}",
                key = object_key(&a.key),
                local = a.local,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("\tvar $$exports = {{ {entries} }};\n"));
}

/// Emit the module-epilogue statement: `customElements.define(tag,
/// $.create_custom_element(Name, props, [], [][, shadow[, extend]]));` for a
/// tagged descriptor, else the bare `$.create_custom_element(…);` statement.
/// The conditional tail: the open/default shadow emits `{ mode: 'open' }`;
/// `shadow: 'none'` OMITS arg5 (spelled `void 0` when an `extend` arg6
/// follows); an object shadow emits its verbatim source; `extend` appends only
/// when given.
pub(super) fn emit_custom_element_epilogue(
    out: &mut String,
    component_name: &str,
    ce: &CustomElementEmission,
) {
    let mut args = format!("{component_name}, {}, [], []", ce.props_object);
    match (&ce.shadow, &ce.extend) {
        (CustomElementShadow::Open, extend) => {
            args.push_str(", { mode: 'open' }");
            if let Some(extend) = extend {
                args.push_str(&format!(", {extend}"));
            }
        }
        (CustomElementShadow::None, Some(extend)) => {
            args.push_str(&format!(", void 0, {extend}"));
        }
        (CustomElementShadow::None, None) => {}
        (CustomElementShadow::ObjectInit(init), extend) => {
            args.push_str(&format!(", {init}"));
            if let Some(extend) = extend {
                args.push_str(&format!(", {extend}"));
            }
        }
    }
    let create = format!("$.create_custom_element({args})");
    match &ce.define_tag {
        Some(tag) => out.push_str(&format!(
            "customElements.define({}, {create});\n",
            js_single_quoted(tag)
        )),
        None => out.push_str(&format!("{create};\n")),
    }
}
