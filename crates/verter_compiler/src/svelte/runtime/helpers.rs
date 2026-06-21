//! The runtime helper vocabulary, the ordered helper trace, the delegated-event
//! set, and the runtime import plan.
//!
//! These types describe the `svelte/internal/{client,server}` helper TOPOLOGY a
//! template needs WITHOUT committing to any emitted `$.`-call string: the
//! [`SvelteHelper`] enum names a helper family, [`HelperTrace`] records the
//! ordered + counted helper references a topology walk plans, [`DelegatedEvents`]
//! holds the first-seen-ordered delegated event-type set, and [`ImportPlan`]
//! captures which side-effect flag imports and which runtime namespace the
//! module imports. The conformant runtime import target is the FIXED
//! `svelte/internal/{client,server}` string — never a configurable runtime
//! module name.
//!
//! The trace keeps BOTH an ordered sequence (a topology walk produces helpers in
//! a deterministic traversal order) AND a per-family count + a cheap membership
//! [`SvelteHelperMask`], so a consumer can compare the planned family SET, the
//! per-family COUNTS, and the traversal-ordered sequence without re-deriving any
//! of them.

use rustc_hash::{FxHashMap, FxHashSet};

/// A `svelte/internal/{client,server}` runtime helper family.
///
/// Each variant names a helper the runtime backends (client / server) emit as a
/// `$.<helper>` call. The semantic IR records WHICH helpers a template's
/// structure needs; the concrete `$.`-call string is a backend concern, so this
/// enum is the boundary between "the topology needs this helper family" and "the
/// backend emits this exact call".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SvelteHelper {
    /// `$.from_html` — the static-HTML template factory.
    FromHtml,
    /// `$.first_child` — descend to a fragment's first child.
    FirstChild,
    /// `$.child` — descend to an element's first child.
    Child,
    /// `$.sibling` — advance to a following sibling (optionally by offset).
    Sibling,
    /// `$.reset` — reset the walk cursor after an element's children.
    Reset,
    /// `$.next` — advance the walk cursor to the next anchor.
    Next,
    /// `$.text` — materialise a text node (optionally seeded).
    Text,
    /// `$.template_effect` — a grouped reactive effect over a template region.
    TemplateEffect,
    /// `$.set_text` — assign a text node's reactive value.
    SetText,
    /// `$.state` — a reactive signal cell.
    State,
    /// `$.proxy` — a deep mutation proxy over an object/array value.
    Proxy,
    /// `$.get` — read a reactive signal.
    Get,
    /// `$.set` — write a reactive signal.
    Set,
    /// `$.update` — in-place increment/decrement of a reactive signal.
    Update,
    /// `$.delegated` — register a delegated event handler on a node.
    Delegated,
    /// `$.delegate` — declare the module's delegated event-type set.
    Delegate,
    /// `$.event` — attach a non-delegated event listener.
    Event,
    /// `$.bind_value` — two-way `bind:value`.
    BindValue,
    /// `$.bind_this` — `bind:this` element/component reference.
    BindThis,
    /// `$.html` — `{@html}` raw-markup insertion.
    Html,
    /// `$.attribute_effect` — a reactive spread-attribute effect.
    AttributeEffect,
    /// `$.append` — mount a fragment/node into an anchor.
    Append,
    /// `$.comment` — a comment-anchor fragment for a block-only / zero-element
    /// root.
    Comment,
    /// `$.if` — an `{#if}` conditional block.
    If,
    /// `$.each` — an `{#each}` list block.
    Each,
    /// `$.index` — the unkeyed-`{#each}` index source sentinel.
    Index,
    /// `$.key` — a `{#key}` keyed re-render block.
    Key,
    /// `$.await` — an `{#await}` promise block.
    Await,
    /// `$.snippet` — a dynamic `{@render expr?.()}` snippet call.
    Snippet,
    /// `$.derived` — a `$derived` / `{@const}` memo.
    Derived,
    /// `$.head` — a `<svelte:head>` region.
    Head,
    /// `$.remove_input_defaults` — strip an `<input>`'s static value/checked
    /// defaults when a `bind:value` / `bind:group` / `bind:checked` is present.
    RemoveInputDefaults,
    /// `$.user_effect` — a `$effect(fn)` user effect.
    UserEffect,
    /// `$.prop` — a `$props()` destructured prop accessor.
    Prop,
    /// `$.push` — open the component instance context (a component using
    /// `$effect` / lifecycle).
    Push,
    /// `$.pop` — close the component instance context.
    Pop,
}

impl SvelteHelper {
    /// The single-bit membership mask for this helper family.
    #[must_use]
    pub const fn bit(self) -> u64 {
        1u64 << (self as u64)
    }

    /// The helper's canonical `$.`-call identifier (the family name without the
    /// `$.` namespace prefix). Used only for diagnostics + topology comparison
    /// against the conformance oracle's helper names; the backends OWN the
    /// emitted call string.
    #[must_use]
    pub const fn ident(self) -> &'static str {
        match self {
            Self::FromHtml => "from_html",
            Self::FirstChild => "first_child",
            Self::Child => "child",
            Self::Sibling => "sibling",
            Self::Reset => "reset",
            Self::Next => "next",
            Self::Text => "text",
            Self::TemplateEffect => "template_effect",
            Self::SetText => "set_text",
            Self::State => "state",
            Self::Proxy => "proxy",
            Self::Get => "get",
            Self::Set => "set",
            Self::Update => "update",
            Self::Delegated => "delegated",
            Self::Delegate => "delegate",
            Self::Event => "event",
            Self::BindValue => "bind_value",
            Self::BindThis => "bind_this",
            Self::Html => "html",
            Self::AttributeEffect => "attribute_effect",
            Self::Append => "append",
            Self::Comment => "comment",
            Self::If => "if",
            Self::Each => "each",
            Self::Index => "index",
            Self::Key => "key",
            Self::Await => "await",
            Self::Snippet => "snippet",
            Self::Derived => "derived",
            Self::Head => "head",
            Self::RemoveInputDefaults => "remove_input_defaults",
            Self::UserEffect => "user_effect",
            Self::Prop => "prop",
            Self::Push => "push",
            Self::Pop => "pop",
        }
    }
}

/// A cheap membership-set over [`SvelteHelper`] families.
///
/// A bitmask companion to [`HelperTrace`]'s ordered sequence + counts: O(1)
/// "does the topology use this helper family?" membership without scanning the
/// sequence. Insufficient ALONE (the conformance oracle checks sequence + counts
/// too), so it is kept ALONGSIDE the sequence, never as a replacement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SvelteHelperMask {
    bits: u64,
}

impl SvelteHelperMask {
    /// The empty mask.
    #[must_use]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Add a helper family to the mask.
    pub fn insert(&mut self, helper: SvelteHelper) {
        self.bits |= helper.bit();
    }

    /// Whether the mask contains a helper family.
    #[must_use]
    pub const fn contains(self, helper: SvelteHelper) -> bool {
        self.bits & helper.bit() != 0
    }

    /// Whether the mask is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }
}

/// The ordered, counted record of helper families a topology walk plans.
///
/// Records every helper reference as the walk produces it: the [`sequence`] is
/// the traversal-ordered list (a topology walk visits nodes in a deterministic
/// order, so the sequence is reproducible), [`counts`] is the per-family
/// occurrence count, and [`mask`] is the cheap membership companion. A consumer
/// compares the family SET (via `mask` / `counts.keys()`), the per-family COUNTS,
/// and the traversal-ordered sequence independently.
///
/// [`sequence`]: HelperTrace::sequence
/// [`counts`]: HelperTrace::counts
/// [`mask`]: HelperTrace::mask
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelperTrace {
    /// The helper families in traversal order (the planner's own recorded
    /// order). This is NOT a claim of official-emission-order parity — it is the
    /// planner's deterministic walk order, which is reproducible and
    /// discriminating.
    pub sequence: Vec<SvelteHelper>,
    /// The per-family occurrence count (a `BTreeMap`-equivalent via a sorted
    /// view; stored as an `FxHashMap` for O(1) increment).
    pub counts: FxHashMap<SvelteHelper, u32>,
    /// The cheap membership companion.
    pub mask: SvelteHelperMask,
}

impl HelperTrace {
    /// A fresh empty trace.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one helper reference: append to the ordered sequence, increment the
    /// per-family count, and add it to the membership mask.
    pub fn call(&mut self, helper: SvelteHelper) {
        self.sequence.push(helper);
        *self.counts.entry(helper).or_insert(0) += 1;
        self.mask.insert(helper);
    }

    /// The sorted unique helper-family SET (the family membership, deterministic
    /// order).
    #[must_use]
    pub fn helper_set(&self) -> Vec<SvelteHelper> {
        let mut set: Vec<SvelteHelper> = self.counts.keys().copied().collect();
        set.sort();
        set
    }

    /// Whether the trace planned the given helper family.
    #[must_use]
    pub fn uses(&self, helper: SvelteHelper) -> bool {
        self.mask.contains(helper)
    }

    /// The occurrence count for a helper family (zero when absent).
    #[must_use]
    pub fn count(&self, helper: SvelteHelper) -> u32 {
        self.counts.get(&helper).copied().unwrap_or(0)
    }
}

/// The ordered, deduplicated set of DELEGATED event types a template registers.
///
/// Modeled exactly as the Vapor backend models delegated events: a `Vec<String>`
/// preserving first-seen order plus an `FxHashSet<String>` for O(1) dedup (there
/// is no `indexmap` dependency). A delegated event is registered per-node via
/// `$.delegated(type, node, handler)`, and the module declares the whole set via
/// a single `$.delegate([...])` call; this type holds that set in the order the
/// types were first seen during the template walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DelegatedEvents {
    /// The event types in first-seen order (`click`, `input`, …).
    order: Vec<String>,
    /// O(1) dedup of the event types already recorded.
    seen: FxHashSet<String>,
}

impl DelegatedEvents {
    /// A fresh empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a delegated event type, preserving first-seen order and
    /// deduplicating. Returns `true` when the type was newly added.
    pub fn register(&mut self, event_type: &str) -> bool {
        if self.seen.insert(event_type.to_string()) {
            self.order.push(event_type.to_string());
            true
        } else {
            false
        }
    }

    /// The event types in first-seen order.
    #[must_use]
    pub fn ordered(&self) -> &[String] {
        &self.order
    }

    /// Whether the set contains an event type.
    #[must_use]
    pub fn contains(&self, event_type: &str) -> bool {
        self.seen.contains(event_type)
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The number of distinct delegated event types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }
}

/// Which fixed `svelte/internal/*` runtime namespace a module imports.
///
/// The conformant import target is a FIXED string per backend — never a
/// configurable runtime module name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeImport {
    /// `import * as $ from 'svelte/internal/client'`.
    Client,
    /// `import * as $ from 'svelte/internal/server'`.
    Server,
}

impl RuntimeImport {
    /// The fixed runtime-namespace module specifier.
    #[must_use]
    pub const fn module_specifier(self) -> &'static str {
        match self {
            Self::Client => "svelte/internal/client",
            Self::Server => "svelte/internal/server",
        }
    }
}

/// The runtime import topology a module needs.
///
/// Captures which side-effect flag imports the module carries
/// (`svelte/internal/disclose-version`, `…/flags/legacy`, `…/flags/async`,
/// `…/flags/tracing`) and which fixed runtime namespace it imports. This is the
/// import-SET plan; the concrete `import` statements are a backend concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportPlan {
    /// Whether `import 'svelte/internal/disclose-version'` is emitted.
    pub disclose_version: bool,
    /// Whether `import 'svelte/internal/flags/legacy'` is emitted (a
    /// non-runes-mode component).
    pub legacy_flag: bool,
    /// Whether `import 'svelte/internal/flags/async'` is emitted (experimental
    /// async in use).
    pub async_flag: bool,
    /// Whether `import 'svelte/internal/flags/tracing'` is emitted (dev-mode
    /// `$inspect.trace`).
    pub tracing_flag: bool,
    /// Which fixed runtime namespace the module imports.
    pub runtime: RuntimeImport,
}

impl ImportPlan {
    /// The default client import plan for a runes-mode, non-async, production
    /// module: the disclose-version side-effect import plus the client runtime
    /// namespace. This mirrors the official two-import preamble for a plain
    /// runes component.
    #[must_use]
    pub const fn client_default() -> Self {
        Self {
            disclose_version: true,
            legacy_flag: false,
            async_flag: false,
            tracing_flag: false,
            runtime: RuntimeImport::Client,
        }
    }

    /// The client import plan for a component in `legacy_mode`: the runes default
    /// plus the `svelte/internal/flags/legacy` side-effect import when the
    /// component is in legacy (non-runes) mode (`import 'svelte/internal/flags/legacy'`
    /// — verified against `svelte@5.56.3`: a store-auto-subscription component
    /// carries the legacy flag, a runes component does not).
    ///
    /// The async / tracing flags stay false: their emission (experimental async,
    /// dev-mode `$inspect.trace`) is a later-block feature this planning foundation
    /// does not yet decide.
    #[must_use]
    pub const fn client_for_mode(legacy_mode: bool) -> Self {
        Self {
            legacy_flag: legacy_mode,
            ..Self::client_default()
        }
    }
}
