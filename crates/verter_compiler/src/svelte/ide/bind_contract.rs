//! The CLOSED Svelte 5 `bind:` contract table — the SOURCE OF TRUTH for the
//! wide binding family (F4).
//!
//! Every Svelte-documented element binding name is pinned here (via the
//! generated [`SVELTE_BIND_CONTRACTS`] table) with its value TYPE and its
//! DIRECTION (read / read-write). The Svelte IDE projector consults this
//! table to emit a type-checked assignment-compatibility check in the projected
//! `.svelte.tsx`. The generic prelude checker is an implementation HELPER — this
//! table is the authority.
//!
//! The table is GENERATED (`scripts/generate-svelte-bind-contract.mjs`) from a
//! closed authored registry and byte-pinned by
//! `crates/verter_compiler/tests/cases/svelte_bind_contract_freshness.rs`, so a
//! registry change without a regen — or a hand-edit of the generated data —
//! fails the gate. The whole-table destructure test below (no `..`) forces a
//! conscious decision on every added binding.

use super::bind_contract_data::SVELTE_BIND_CONTRACTS;

/// The binding direction, from the bound LOCAL's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindDirection {
    /// Read-write: Svelte reads the local to set the DOM AND writes DOM changes
    /// back into the local — the local is INVARIANT with the value type.
    ReadWrite,
    /// Read-direction (readonly DOM property, DOM → local only): the local
    /// RECEIVES the value from the DOM and can never write back — the value type
    /// must be assignable to the local, and a userland write to the binding
    /// target is rejected.
    Read,
}

/// A binding that routes to a DEDICATED checker rather than the generic
/// value-type assignment-compat check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindSpecial {
    /// `bind:this` — host-instance assignment-compat (the projector substitutes
    /// the element's host-instance type and routes to the `this` checker).
    This,
    /// `bind:group` — checkbox-vs-radio shared selection (the projector inspects
    /// the sibling `type` attribute and routes to the radio/checkbox checker).
    Group,
    /// No special routing — the generic value-type check applies.
    None,
}

/// One row of the closed bind-contract table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindContract {
    /// The binding local (`value` in `bind:value`).
    pub name: &'static str,
    /// The binding direction.
    pub direction: BindDirection,
    /// The TS type of the bound value (a `svelte`/DOM type expression). For
    /// host-instance bindings (`bind:this`) the literal `{HOST}` placeholder is
    /// substituted by the projector with the element's host-instance type.
    pub value_type: &'static str,
    /// The applicable lowercase tag set (comma-separated), `*` for any element,
    /// or `contenteditable` (documentary — any element carrying the attribute).
    pub tags: &'static str,
    /// The dedicated-checker routing marker, if any.
    pub special: BindSpecial,
}

impl BindContract {
    /// Whether this contract's `tags` constraint admits the given lowercase tag.
    /// `*` and `contenteditable` admit any element (the projector does not
    /// enforce the contenteditable attribute presence — a userland mismatch is a
    /// rare authoring error, and the value type still checks).
    #[must_use]
    pub fn applies_to_tag(&self, tag: &str) -> bool {
        match self.tags {
            "*" | "contenteditable" => true,
            list => list.split(',').any(|t| t == tag),
        }
    }
}

/// Look up the bind contract for `name` that applies to the lowercase `tag`.
///
/// A name may appear once in the table with a tag constraint; the lookup
/// returns the row only when the tag is admitted. A name absent from the table
/// (or present but not admitted for `tag`) returns `None` — the projector then
/// treats it as an unknown binding (no F4 contract). The bind names that resolve
/// through the plain JSX intrinsic table (`value`, `checked`) and through
/// `SvelteHTMLElements` attributes (`defaultValue`, `defaultChecked`) are
/// DELIBERATELY ABSENT here — they are not wide-family contracts.
#[must_use]
pub fn lookup_bind_contract(name: &str, tag: &str) -> Option<&'static BindContract> {
    SVELTE_BIND_CONTRACTS
        .iter()
        .find(|c| c.name == name && c.applies_to_tag(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole-table destructure test (NO `..`). It binds every field of every
    /// row, so ADDING a binding to the registry forces a conscious decision here
    /// (the match arm for the new name must be added or this test fails to
    /// compile / fails its assertion). It also pins each row's direction +
    /// special routing so a registry edit that silently flips a direction is
    /// caught.
    #[test]
    fn every_bind_contract_row_is_consciously_accounted_for() {
        for contract in SVELTE_BIND_CONTRACTS {
            // Destructure WITHOUT `..` — a new field forces this to be updated.
            let BindContract {
                name,
                direction,
                value_type,
                tags,
                special,
            } = contract;

            assert!(!name.is_empty(), "binding name is non-empty");
            assert!(!value_type.is_empty(), "value type is non-empty: {name}");
            assert!(!tags.is_empty(), "tags is non-empty: {name}");

            // The closed expected (direction, special) for EVERY documented
            // binding name — a `..`-free exhaustive match forces a conscious
            // decision on any added name (a new name hits the wildcard panic).
            let (expected_dir, expected_special) = match *name {
                "this" => (BindDirection::Read, BindSpecial::This),
                "group" => (BindDirection::ReadWrite, BindSpecial::Group),
                "files" => (BindDirection::ReadWrite, BindSpecial::None),
                "indeterminate" => (BindDirection::ReadWrite, BindSpecial::None),
                "open" => (BindDirection::ReadWrite, BindSpecial::None),
                "innerHTML" | "innerText" | "textContent" => {
                    (BindDirection::ReadWrite, BindSpecial::None)
                }
                "currentTime" | "playbackRate" | "volume" | "muted" | "paused" => {
                    (BindDirection::ReadWrite, BindSpecial::None)
                }
                "duration" | "buffered" | "seekable" | "played" | "seeking" | "ended"
                | "readyState" => (BindDirection::Read, BindSpecial::None),
                "clientWidth" | "clientHeight" | "offsetWidth" | "offsetHeight" => {
                    (BindDirection::Read, BindSpecial::None)
                }
                "naturalWidth" | "naturalHeight" | "videoWidth" | "videoHeight" => {
                    (BindDirection::Read, BindSpecial::None)
                }
                "contentRect"
                | "contentBoxSize"
                | "borderBoxSize"
                | "devicePixelContentBoxSize" => (BindDirection::Read, BindSpecial::None),
                other => panic!(
                    "unaccounted bind-contract row `{other}` — add it to the \
                     whole-table destructure test with its conscious \
                     direction/special decision"
                ),
            };
            assert_eq!(*direction, expected_dir, "direction for `{name}`");
            assert_eq!(*special, expected_special, "special for `{name}`");
        }
    }

    #[test]
    fn lookup_respects_tag_constraints() {
        // `bind:open` is `<details>`-scoped.
        assert!(lookup_bind_contract("open", "details").is_some());
        assert!(lookup_bind_contract("open", "div").is_none());
        // Media bindings are `<audio>`/`<video>`-scoped.
        assert!(lookup_bind_contract("currentTime", "video").is_some());
        assert!(lookup_bind_contract("currentTime", "div").is_none());
        // Dimension bindings + `bind:this` apply to any element.
        assert!(lookup_bind_contract("clientWidth", "div").is_some());
        assert!(lookup_bind_contract("this", "span").is_some());
        // `value`/`checked` are NOT wide-family contracts (they go through the
        // plain JSX intrinsic attribute path).
        assert!(lookup_bind_contract("value", "input").is_none());
        assert!(lookup_bind_contract("checked", "input").is_none());
    }

    #[test]
    fn readonly_bindings_carry_the_read_direction() {
        // The readonly DOM properties are read-direction (a userland write to the
        // binding target is rejected by the projected `r`-mode check).
        for name in ["duration", "clientWidth", "naturalWidth"] {
            let c = SVELTE_BIND_CONTRACTS
                .iter()
                .find(|c| c.name == name)
                .unwrap();
            assert_eq!(c.direction, BindDirection::Read, "{name} is read-direction");
        }
    }
}
