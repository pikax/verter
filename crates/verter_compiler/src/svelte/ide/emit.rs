//! Small projection helpers shared by the Svelte IDE projector.
//!
//! These are pure text/transform utilities — no type lowering, no resolution.

/// A machine-stable diagnostic code for an OUT-OF-SCOPE matrix construct.
///
/// The projector emits the construct's expressions void-checked and records
/// one of these codes so the session surfaces a typed-unsupported diagnostic
/// (never a crash, never a silent drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedKind {
    /// An await-expression (`{#await}` arms use experimental async syntax in
    /// 5.36+). D-bg: parse-without-crash + void-check + the diagnostic.
    AwaitExperimental,
    /// A `<svelte:component>` / `<svelte:self>` deprecated special element.
    DeprecatedSpecialElement,
    /// A `<svelte:fragment>` legacy slot construct.
    LegacyFragment,
    /// A function-binding `bind:x={get, set}` (out-of-scope v1).
    FunctionBinding,
    /// An out-of-scope `bind:*` family member beyond the supported set
    /// (`bind:value`, `bind:checked`, and component `bind:prop`). Covers
    /// `bind:this`, `bind:group`, `bind:files`, and the contenteditable, media,
    /// dimension, and readonly bindings. The diagnostic NAMES the binding (the
    /// matrix requires a "typed-unsupported diagnostic naming the binding").
    UnsupportedBinding,
    /// An unrecognised construct parsed without crash.
    Unknown,
}

impl UnsupportedKind {
    /// The machine-stable diagnostic code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::AwaitExperimental => "svelte-await-experimental",
            Self::DeprecatedSpecialElement => "svelte-deprecated-special-element",
            Self::LegacyFragment => "svelte-legacy-fragment",
            Self::FunctionBinding => "svelte-function-binding",
            Self::UnsupportedBinding => "svelte-unsupported-binding",
            Self::Unknown => "svelte-unsupported-construct",
        }
    }

    /// A human-readable message.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::AwaitExperimental => {
                "await-expressions are experimental (Svelte 5.36+, `experimental.async`) and out of scope"
            }
            Self::DeprecatedSpecialElement => {
                "`<svelte:component>` / `<svelte:self>` are deprecated in runes mode and out of scope"
            }
            Self::LegacyFragment => "`<svelte:fragment>` is a legacy slot construct, out of scope",
            Self::FunctionBinding => {
                "function bindings `bind:x={get, set}` are out of scope (v1)"
            }
            Self::UnsupportedBinding => {
                "this `bind:` binding is out of scope (v1); only `bind:value`, \
                 `bind:checked`, and component `bind:prop` for `$bindable` props \
                 are supported"
            }
            Self::Unknown => "unsupported Svelte construct",
        }
    }
}

/// Whether an attribute name is a CSS custom property (`--x`) — these are
/// stripped from the projected JSX attribute position (D-ap), their value
/// void-checked.
#[must_use]
pub fn is_css_custom_property(name: &str) -> bool {
    name.starts_with("--")
}
