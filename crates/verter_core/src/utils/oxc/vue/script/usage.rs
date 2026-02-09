//! Vue Composition API usage detection and collection.
//!
//! This module provides byte-based detection of Vue Composition API calls
//! like provide, inject, ref, computed, lifecycle hooks, etc., and types
//! to represent usage information for cross-file static analysis.

#![allow(dead_code)]

use crate::common::Span;

// =============================================================================
// Vue API Kind Detection
// =============================================================================

/// Vue Composition API function kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VueApiKind {
    // Dependency Injection
    Provide = 0,
    Inject = 1,

    // Reactivity: Core
    Ref = 2,
    Reactive = 3,
    Computed = 4,

    // Reactivity: Shallow
    ShallowRef = 5,
    ShallowReactive = 6,

    // Watchers
    Watch = 7,
    WatchEffect = 8,
    WatchPostEffect = 9,
    WatchSyncEffect = 10,

    // Lifecycle Hooks
    OnMounted = 11,
    OnUnmounted = 12,
    OnBeforeMount = 13,
    OnBeforeUnmount = 14,
    OnUpdated = 15,
    OnBeforeUpdate = 16,
    OnErrorCaptured = 17,
    OnActivated = 18,
    OnDeactivated = 19,
    OnRenderTracked = 20,
    OnRenderTriggered = 21,
    OnServerPrefetch = 22,

    // Template Utilities
    UseSlots = 23,
    UseAttrs = 24,
    UseTemplateRef = 25,

    // Instance Access (requires sync context)
    GetCurrentInstance = 26,
}

impl VueApiKind {
    /// Get the category of this API
    pub const fn category(&self) -> VueApiCategory {
        match self {
            Self::Provide | Self::Inject => VueApiCategory::DependencyInjection,
            Self::Ref
            | Self::Reactive
            | Self::Computed
            | Self::ShallowRef
            | Self::ShallowReactive => VueApiCategory::Reactivity,
            Self::Watch | Self::WatchEffect | Self::WatchPostEffect | Self::WatchSyncEffect => {
                VueApiCategory::Watchers
            }
            Self::OnMounted
            | Self::OnUnmounted
            | Self::OnBeforeMount
            | Self::OnBeforeUnmount
            | Self::OnUpdated
            | Self::OnBeforeUpdate
            | Self::OnErrorCaptured
            | Self::OnActivated
            | Self::OnDeactivated
            | Self::OnRenderTracked
            | Self::OnRenderTriggered
            | Self::OnServerPrefetch => VueApiCategory::Lifecycle,
            Self::UseSlots | Self::UseAttrs | Self::UseTemplateRef => VueApiCategory::TemplateUtils,
            Self::GetCurrentInstance => VueApiCategory::InstanceAccess,
        }
    }

    /// Get a static description of this API
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Provide => "provide() dependency injection",
            Self::Inject => "inject() dependency injection",
            Self::Ref => "ref() reactive reference",
            Self::Reactive => "reactive() reactive object",
            Self::Computed => "computed() computed property",
            Self::ShallowRef => "shallowRef() shallow reactive reference",
            Self::ShallowReactive => "shallowReactive() shallow reactive object",
            Self::Watch => "watch() watcher",
            Self::WatchEffect => "watchEffect() effect watcher",
            Self::WatchPostEffect => "watchPostEffect() post-render effect",
            Self::WatchSyncEffect => "watchSyncEffect() synchronous effect",
            Self::OnMounted => "onMounted() lifecycle hook",
            Self::OnUnmounted => "onUnmounted() lifecycle hook",
            Self::OnBeforeMount => "onBeforeMount() lifecycle hook",
            Self::OnBeforeUnmount => "onBeforeUnmount() lifecycle hook",
            Self::OnUpdated => "onUpdated() lifecycle hook",
            Self::OnBeforeUpdate => "onBeforeUpdate() lifecycle hook",
            Self::OnErrorCaptured => "onErrorCaptured() lifecycle hook",
            Self::OnActivated => "onActivated() keep-alive hook",
            Self::OnDeactivated => "onDeactivated() keep-alive hook",
            Self::OnRenderTracked => "onRenderTracked() debug hook",
            Self::OnRenderTriggered => "onRenderTriggered() debug hook",
            Self::OnServerPrefetch => "onServerPrefetch() SSR hook",
            Self::UseSlots => "useSlots() slot access",
            Self::UseAttrs => "useAttrs() attribute access",
            Self::UseTemplateRef => "useTemplateRef() template ref access",
            Self::GetCurrentInstance => {
                "getCurrentInstance() instance access (sync context required)"
            }
        }
    }

    /// Check if this API requires synchronous setup/lifecycle context
    pub const fn requires_sync_context(&self) -> bool {
        matches!(self, Self::GetCurrentInstance)
    }
}

/// Categories of Vue Composition API functions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VueApiCategory {
    DependencyInjection,
    Reactivity,
    Watchers,
    Lifecycle,
    TemplateUtils,
    /// APIs that require synchronous setup/lifecycle context
    InstanceAccess,
}

/// Check if a byte slice is a Vue Composition API function and return the kind.
///
/// Uses length-based filtering and first-byte dispatch for O(1) rejection.
#[inline]
pub fn detect_vue_api_call(name: &[u8]) -> Option<VueApiKind> {
    let len = name.len();

    // Vue API functions range from 3 to 18 characters
    // Shortest: ref (3), Longest: getCurrentInstance (18)
    if !(3..=18).contains(&len) {
        return None;
    }

    // First-byte dispatch for fast rejection
    match name.first()? {
        b'p' => detect_p_api(name, len),
        b'i' => detect_i_api(name, len),
        b'r' => detect_r_api(name, len),
        b'c' => detect_c_api(name, len),
        b's' => detect_s_api(name, len),
        b'w' => detect_w_api(name, len),
        b'o' => detect_o_api(name, len),
        b'u' => detect_u_api(name, len),
        b'g' => detect_g_api(name, len),
        _ => None,
    }
}

// Detect APIs starting with 'p': provide
fn detect_p_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    if len == 7 && name == b"provide" {
        Some(VueApiKind::Provide)
    } else {
        None
    }
}

// Detect APIs starting with 'i': inject
fn detect_i_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    if len == 6 && name == b"inject" {
        Some(VueApiKind::Inject)
    } else {
        None
    }
}

// Detect APIs starting with 'r': ref, reactive
fn detect_r_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    match len {
        3 if name == b"ref" => Some(VueApiKind::Ref),
        8 if name == b"reactive" => Some(VueApiKind::Reactive),
        _ => None,
    }
}

// Detect APIs starting with 'c': computed
fn detect_c_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    if len == 8 && name == b"computed" {
        Some(VueApiKind::Computed)
    } else {
        None
    }
}

// Detect APIs starting with 's': shallowRef, shallowReactive
fn detect_s_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    match len {
        10 if name == b"shallowRef" => Some(VueApiKind::ShallowRef),
        15 if name == b"shallowReactive" => Some(VueApiKind::ShallowReactive),
        _ => None,
    }
}

// Detect APIs starting with 'w': watch, watchEffect, watchPostEffect, watchSyncEffect
fn detect_w_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    match len {
        5 if name == b"watch" => Some(VueApiKind::Watch),
        11 if name == b"watchEffect" => Some(VueApiKind::WatchEffect),
        15 if name == b"watchPostEffect" => Some(VueApiKind::WatchPostEffect),
        15 if name == b"watchSyncEffect" => Some(VueApiKind::WatchSyncEffect),
        _ => None,
    }
}

// Detect APIs starting with 'o': lifecycle hooks
fn detect_o_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    match len {
        9 => {
            if name == b"onMounted" {
                Some(VueApiKind::OnMounted)
            } else if name == b"onUpdated" {
                Some(VueApiKind::OnUpdated)
            } else {
                None
            }
        }
        11 => {
            if name == b"onUnmounted" {
                Some(VueApiKind::OnUnmounted)
            } else if name == b"onActivated" {
                Some(VueApiKind::OnActivated)
            } else {
                None
            }
        }
        13 => {
            if name == b"onBeforeMount" {
                Some(VueApiKind::OnBeforeMount)
            } else if name == b"onDeactivated" {
                Some(VueApiKind::OnDeactivated)
            } else {
                None
            }
        }
        14 => {
            if name == b"onBeforeUpdate" {
                Some(VueApiKind::OnBeforeUpdate)
            } else {
                None
            }
        }
        15 => {
            if name == b"onBeforeUnmount" {
                Some(VueApiKind::OnBeforeUnmount)
            } else if name == b"onErrorCaptured" {
                Some(VueApiKind::OnErrorCaptured)
            } else if name == b"onRenderTracked" {
                Some(VueApiKind::OnRenderTracked)
            } else {
                None
            }
        }
        16 if name == b"onServerPrefetch" => Some(VueApiKind::OnServerPrefetch),
        17 if name == b"onRenderTriggered" => Some(VueApiKind::OnRenderTriggered),
        _ => None,
    }
}

// Detect APIs starting with 'u': useSlots, useAttrs, useTemplateRef
fn detect_u_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    match len {
        8 => {
            if name == b"useSlots" {
                Some(VueApiKind::UseSlots)
            } else if name == b"useAttrs" {
                Some(VueApiKind::UseAttrs)
            } else {
                None
            }
        }
        14 if name == b"useTemplateRef" => Some(VueApiKind::UseTemplateRef),
        _ => None,
    }
}

// Detect APIs starting with 'g': getCurrentInstance
fn detect_g_api(name: &[u8], len: usize) -> Option<VueApiKind> {
    if len == 18 && name == b"getCurrentInstance" {
        Some(VueApiKind::GetCurrentInstance)
    } else {
        None
    }
}

// =============================================================================
// Usage Entry Types
// =============================================================================

/// Type of injection key
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvideKeyKind {
    /// String literal key: provide('key', value)
    StringLiteral,
    /// Symbol or identifier key: provide(InjectionKey, value)
    Symbol,
    /// Dynamic/unknown key: provide(someVar, value)
    Dynamic,
}

/// A provide/inject key with its span
#[derive(Debug, Clone)]
pub struct ProvideKey {
    /// Span of the key expression
    pub span: Span,
    /// What kind of key this is
    pub kind: ProvideKeyKind,
}

/// A provide() call usage
#[derive(Debug, Clone)]
pub struct ProvideUsage {
    /// Span of the entire provide() call
    pub span: Span,
    /// The injection key
    pub key: ProvideKey,
    /// Span of the provided value expression
    pub value_span: Span,
}

/// An inject() call usage
#[derive(Debug, Clone)]
pub struct InjectUsage {
    /// Span of the entire inject() call
    pub span: Span,
    /// The injection key
    pub key: ProvideKey,
    /// Whether a default value was provided
    pub has_default: bool,
    /// The binding name span (if assigned: const foo = inject(...))
    pub binding_span: Option<Span>,
}

/// Lifecycle hook kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LifecycleHook {
    OnMounted,
    OnUnmounted,
    OnBeforeMount,
    OnBeforeUnmount,
    OnUpdated,
    OnBeforeUpdate,
    OnErrorCaptured,
    OnActivated,
    OnDeactivated,
    OnRenderTracked,
    OnRenderTriggered,
    OnServerPrefetch,
}

impl LifecycleHook {
    pub const fn from_api_kind(kind: VueApiKind) -> Option<Self> {
        match kind {
            VueApiKind::OnMounted => Some(Self::OnMounted),
            VueApiKind::OnUnmounted => Some(Self::OnUnmounted),
            VueApiKind::OnBeforeMount => Some(Self::OnBeforeMount),
            VueApiKind::OnBeforeUnmount => Some(Self::OnBeforeUnmount),
            VueApiKind::OnUpdated => Some(Self::OnUpdated),
            VueApiKind::OnBeforeUpdate => Some(Self::OnBeforeUpdate),
            VueApiKind::OnErrorCaptured => Some(Self::OnErrorCaptured),
            VueApiKind::OnActivated => Some(Self::OnActivated),
            VueApiKind::OnDeactivated => Some(Self::OnDeactivated),
            VueApiKind::OnRenderTracked => Some(Self::OnRenderTracked),
            VueApiKind::OnRenderTriggered => Some(Self::OnRenderTriggered),
            VueApiKind::OnServerPrefetch => Some(Self::OnServerPrefetch),
            _ => None,
        }
    }
}

/// A lifecycle hook usage
#[derive(Debug, Clone)]
pub struct LifecycleUsage {
    /// Span of the entire hook call
    pub span: Span,
    /// Which lifecycle hook
    pub hook: LifecycleHook,
    /// Span of the callback function
    pub callback_span: Span,
}

/// Kind of reactive state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactiveKind {
    Ref,
    ShallowRef,
    Reactive,
    ShallowReactive,
    Computed,
}

impl ReactiveKind {
    pub const fn from_api_kind(kind: VueApiKind) -> Option<Self> {
        match kind {
            VueApiKind::Ref => Some(Self::Ref),
            VueApiKind::ShallowRef => Some(Self::ShallowRef),
            VueApiKind::Reactive => Some(Self::Reactive),
            VueApiKind::ShallowReactive => Some(Self::ShallowReactive),
            VueApiKind::Computed => Some(Self::Computed),
            _ => None,
        }
    }
}

/// A reactive state definition
#[derive(Debug, Clone)]
pub struct ReactiveStateUsage {
    /// Kind of reactive state
    pub kind: ReactiveKind,
    /// Span of the binding name (e.g., "count" in const count = ref(0))
    pub binding_span: Span,
    /// Span of the initializer expression (if any)
    pub initializer_span: Option<Span>,
}

/// A watcher usage (watch, watchEffect, etc.)
#[derive(Debug, Clone)]
pub struct WatcherUsage {
    /// Span of the entire watcher call
    pub span: Span,
    /// Kind of watcher (Watch, WatchEffect, etc.)
    pub kind: VueApiKind,
    /// Span of the callback/effect function
    pub callback_span: Span,
    /// For watch(): spans of watched sources
    pub source_spans: Vec<Span>,
}

/// Event name for emit calls
#[derive(Debug, Clone)]
pub enum EmitEventName {
    /// Static string: emit('eventName', ...)
    Static { span: Span },
    /// Dynamic: emit(dynamicName, ...)
    Dynamic { span: Span },
}

/// An emit() call usage
#[derive(Debug, Clone)]
pub struct EmitCallUsage {
    /// Span of the entire emit() call
    pub span: Span,
    /// The event name
    pub event_name: EmitEventName,
    /// Spans of argument expressions (payload)
    pub arg_spans: Vec<Span>,
}

/// A useSlots/useAttrs/useTemplateRef usage
#[derive(Debug, Clone)]
pub struct TemplateUtilUsage {
    /// Span of the entire call
    pub span: Span,
    /// Which utility function
    pub kind: VueApiKind,
    /// The binding name span (if assigned)
    pub binding_span: Option<Span>,
    /// For useTemplateRef: the ref name argument span
    pub ref_name_span: Option<Span>,
}

// =============================================================================
// Sync Context Tracking (getCurrentInstance safety)
// =============================================================================

/// Call site context for APIs that require synchronous setup/lifecycle context.
///
/// This tracks whether calls like `getCurrentInstance()` are made in safe contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSiteContext {
    /// Before any await statement - SAFE
    /// getCurrentInstance() will return the component instance
    BeforeAwait,

    /// After an await statement at top level - POTENTIALLY UNSAFE
    /// The component may have been unmounted between await and this call
    AfterAwait,

    /// Inside a lifecycle hook callback (onMounted, etc.) - SAFE
    /// The callback runs in component context
    InLifecycleCallback,

    /// Inside a computed/watch callback - SAFE
    /// These run in component context
    InReactiveCallback,

    /// Inside setTimeout/setInterval callback - UNSAFE
    /// The callback runs outside component context
    InTimerCallback,

    /// Inside Promise.then/catch/finally callback - POTENTIALLY UNSAFE
    /// Unless the promise is awaited
    InPromiseCallback,

    /// Context couldn't be determined statically
    Unknown,
}

impl CallSiteContext {
    /// Check if this context is definitely safe for sync-context APIs
    pub const fn is_safe(&self) -> bool {
        matches!(
            self,
            Self::BeforeAwait | Self::InLifecycleCallback | Self::InReactiveCallback
        )
    }

    /// Check if this context is definitely unsafe
    pub const fn is_unsafe(&self) -> bool {
        matches!(self, Self::InTimerCallback)
    }

    /// Check if this context is potentially problematic
    pub const fn is_potentially_unsafe(&self) -> bool {
        matches!(self, Self::AfterAwait | Self::InPromiseCallback)
    }

    /// Get a human-readable description
    pub const fn description(&self) -> &'static str {
        match self {
            Self::BeforeAwait => "before any await (safe)",
            Self::AfterAwait => "after await (potentially unsafe)",
            Self::InLifecycleCallback => "inside lifecycle hook (safe)",
            Self::InReactiveCallback => "inside computed/watch (safe)",
            Self::InTimerCallback => "inside setTimeout/setInterval (unsafe)",
            Self::InPromiseCallback => "inside Promise callback (potentially unsafe)",
            Self::Unknown => "unknown context",
        }
    }
}

/// Usage of an API that requires synchronous setup/lifecycle context.
///
/// Currently tracks `getCurrentInstance()` calls with context about whether
/// the call is in a safe position.
#[derive(Debug, Clone)]
pub struct SyncContextUsage {
    /// Span of the entire call
    pub span: Span,
    /// Which API (currently only GetCurrentInstance)
    pub kind: VueApiKind,
    /// The detected call site context
    pub context: CallSiteContext,
    /// The binding name span (if assigned: const instance = getCurrentInstance())
    pub binding_span: Option<Span>,
    /// If AfterAwait, the span of the first await that preceded this call
    pub preceding_await_span: Option<Span>,
}

impl SyncContextUsage {
    /// Check if this usage is in a safe context
    pub fn is_safe(&self) -> bool {
        self.context.is_safe()
    }

    /// Check if this usage is potentially problematic
    pub fn is_potentially_unsafe(&self) -> bool {
        self.context.is_potentially_unsafe() || self.context.is_unsafe()
    }
}

// =============================================================================
// Usage Flags
// =============================================================================

/// Bit flags for quick "has X" queries
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileUsageFlags {
    bits: u32,
}

impl FileUsageFlags {
    // Script usage flags
    pub const HAS_PROVIDE: u32 = 1 << 0;
    pub const HAS_INJECT: u32 = 1 << 1;
    pub const HAS_LIFECYCLE_HOOKS: u32 = 1 << 2;
    pub const HAS_REACTIVE_STATE: u32 = 1 << 3;
    pub const HAS_WATCHERS: u32 = 1 << 4;
    pub const HAS_EMIT_CALLS: u32 = 1 << 5;
    pub const HAS_TEMPLATE_UTILS: u32 = 1 << 6;
    pub const IS_ASYNC_SETUP: u32 = 1 << 7;

    // Template usage flags
    pub const HAS_TEMPLATE_REFS: u32 = 1 << 8;
    pub const HAS_SLOT_USAGE: u32 = 1 << 9;
    pub const HAS_COMPONENT_USAGE: u32 = 1 << 10;
    pub const HAS_SLOT_DEFINITIONS: u32 = 1 << 11;

    // Sync context flags
    pub const HAS_SYNC_CONTEXT_USAGE: u32 = 1 << 12;
    pub const HAS_UNSAFE_SYNC_CONTEXT: u32 = 1 << 13;

    /// Set a flag
    #[inline]
    pub fn set(&mut self, flag: u32) {
        self.bits |= flag;
    }

    /// Check if a flag is set
    #[inline]
    pub const fn has(&self, flag: u32) -> bool {
        (self.bits & flag) != 0
    }

    /// Get raw bits
    #[inline]
    pub const fn bits(&self) -> u32 {
        self.bits
    }
}

// =============================================================================
// Usage Collector
// =============================================================================

/// Context for collecting Vue API usage during script traversal.
/// Passed alongside SetupContext during processing.
#[derive(Debug)]
pub struct UsageCollector<'a> {
    /// Provide calls
    pub provides: Vec<ProvideUsage>,
    /// Inject calls
    pub injects: Vec<InjectUsage>,
    /// Lifecycle hooks
    pub lifecycle: Vec<LifecycleUsage>,
    /// Reactive state definitions
    pub reactive: Vec<ReactiveStateUsage>,
    /// Watcher definitions
    pub watchers: Vec<WatcherUsage>,
    /// Emit calls
    pub emit_calls: Vec<EmitCallUsage>,
    /// Template utility usage (useSlots, useAttrs, useTemplateRef)
    pub template_utils: Vec<TemplateUtilUsage>,
    /// Sync context API usage (getCurrentInstance)
    pub sync_context_usages: Vec<SyncContextUsage>,

    /// Quick lookup flags
    pub flags: FileUsageFlags,

    /// Source bytes for span extraction (if needed)
    _source: &'a [u8],

    /// Span of the first await encountered (for tracking before/after)
    first_await_span: Option<Span>,
}

impl<'a> UsageCollector<'a> {
    /// Create a new usage collector with default capacity hints
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            provides: Vec::with_capacity(2),
            injects: Vec::with_capacity(4),
            lifecycle: Vec::with_capacity(4),
            reactive: Vec::with_capacity(8),
            watchers: Vec::with_capacity(4),
            emit_calls: Vec::with_capacity(4),
            template_utils: Vec::with_capacity(2),
            sync_context_usages: Vec::with_capacity(2),
            flags: FileUsageFlags::default(),
            _source: source,
            first_await_span: None,
        }
    }

    /// Create with size-based capacity hints
    pub fn with_size_hint(source: &'a [u8]) -> Self {
        let len = source.len();

        // Heuristic: larger files tend to have more of everything
        let (reactive_cap, lifecycle_cap, watcher_cap) = if len < 1000 {
            (4, 2, 2)
        } else if len < 5000 {
            (8, 4, 4)
        } else {
            (16, 8, 8)
        };

        Self {
            provides: Vec::with_capacity(2),
            injects: Vec::with_capacity(4),
            lifecycle: Vec::with_capacity(lifecycle_cap),
            reactive: Vec::with_capacity(reactive_cap),
            watchers: Vec::with_capacity(watcher_cap),
            emit_calls: Vec::with_capacity(4),
            template_utils: Vec::with_capacity(2),
            sync_context_usages: Vec::with_capacity(2),
            flags: FileUsageFlags::default(),
            _source: source,
            first_await_span: None,
        }
    }

    /// Record that an await was encountered at the given span
    #[inline]
    pub fn record_await(&mut self, span: Span) {
        if self.first_await_span.is_none() {
            self.first_await_span = Some(span);
        }
    }

    /// Check if we've seen an await before the given position
    #[inline]
    pub fn has_await_before(&self, pos: u32) -> bool {
        self.first_await_span.is_some_and(|s| s.end < pos)
    }

    /// Get the first await span if any
    #[inline]
    pub fn first_await_span(&self) -> Option<Span> {
        self.first_await_span
    }

    /// Record a provide() call
    #[inline]
    pub fn record_provide(&mut self, usage: ProvideUsage) {
        self.flags.set(FileUsageFlags::HAS_PROVIDE);
        self.provides.push(usage);
    }

    /// Record an inject() call
    #[inline]
    pub fn record_inject(&mut self, usage: InjectUsage) {
        self.flags.set(FileUsageFlags::HAS_INJECT);
        self.injects.push(usage);
    }

    /// Record a lifecycle hook
    #[inline]
    pub fn record_lifecycle(&mut self, usage: LifecycleUsage) {
        self.flags.set(FileUsageFlags::HAS_LIFECYCLE_HOOKS);
        self.lifecycle.push(usage);
    }

    /// Record a reactive state definition
    #[inline]
    pub fn record_reactive(&mut self, usage: ReactiveStateUsage) {
        self.flags.set(FileUsageFlags::HAS_REACTIVE_STATE);
        self.reactive.push(usage);
    }

    /// Record a watcher
    #[inline]
    pub fn record_watcher(&mut self, usage: WatcherUsage) {
        self.flags.set(FileUsageFlags::HAS_WATCHERS);
        self.watchers.push(usage);
    }

    /// Record an emit call
    #[inline]
    pub fn record_emit(&mut self, usage: EmitCallUsage) {
        self.flags.set(FileUsageFlags::HAS_EMIT_CALLS);
        self.emit_calls.push(usage);
    }

    /// Record a template utility usage
    #[inline]
    pub fn record_template_util(&mut self, usage: TemplateUtilUsage) {
        self.flags.set(FileUsageFlags::HAS_TEMPLATE_UTILS);
        self.template_utils.push(usage);
    }

    /// Record a sync context API usage (getCurrentInstance)
    #[inline]
    pub fn record_sync_context_usage(&mut self, usage: SyncContextUsage) {
        self.flags.set(FileUsageFlags::HAS_SYNC_CONTEXT_USAGE);
        if usage.is_potentially_unsafe() {
            self.flags.set(FileUsageFlags::HAS_UNSAFE_SYNC_CONTEXT);
        }
        self.sync_context_usages.push(usage);
    }

    /// Check if any usage was collected
    pub fn is_empty(&self) -> bool {
        self.flags.bits() == 0
    }

    /// Check if there are any potentially unsafe sync context usages
    pub fn has_unsafe_sync_context(&self) -> bool {
        self.flags.has(FileUsageFlags::HAS_UNSAFE_SYNC_CONTEXT)
    }

    /// Get all sync context usages that are potentially unsafe
    pub fn unsafe_sync_context_usages(&self) -> impl Iterator<Item = &SyncContextUsage> {
        self.sync_context_usages
            .iter()
            .filter(|u| u.is_potentially_unsafe())
    }
}

// =============================================================================
// Template Usage Types
// =============================================================================

/// A template ref attribute (ref="name" or :ref="dynamic")
#[derive(Debug, Clone)]
pub struct TemplateRefAttrUsage {
    /// Span of the ref name value
    pub name_span: Span,
    /// Element ID where this ref is defined
    pub element_id: u32,
    /// Whether this is a dynamic ref (:ref="...")
    pub is_dynamic: bool,
}

/// Slot name types
#[derive(Debug, Clone)]
pub enum SlotName {
    /// Default slot (no name)
    Default,
    /// Named slot: name="header" or #header
    Named { span: Span },
    /// Dynamic slot: #[dynamicName] or v-slot:[dynamicName]
    Dynamic { span: Span },
}

/// A slot usage in template (v-slot directive)
#[derive(Debug, Clone)]
pub struct SlotUsageInfo {
    /// Span of the entire v-slot directive
    pub span: Span,
    /// The slot name
    pub name: SlotName,
    /// Element ID where this slot is used
    pub element_id: u32,
    /// Scope bindings provided by the slot (v-slot="{ item }")
    pub scope_binding_spans: Vec<Span>,
}

/// A slot definition (<slot> element)
#[derive(Debug, Clone)]
pub struct SlotDefinitionInfo {
    /// Span of the slot element
    pub span: Span,
    /// Slot name (None for default slot)
    pub name_span: Option<Span>,
    /// Element ID
    pub element_id: u32,
}

/// A component usage in template (<MyComponent>)
#[derive(Debug, Clone)]
pub struct ComponentUsageInfo {
    /// Span of the component tag
    pub span: Span,
    /// Span of the component name
    pub name_span: Span,
    /// Element ID
    pub element_id: u32,
    /// Whether this is a dynamic component (<component :is="...">)
    pub is_dynamic: bool,
}

/// Context of a binding reference in template
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingRefContext {
    /// Inside {{ interpolation }}
    Interpolation,
    /// In directive value (v-if="binding")
    DirectiveValue,
    /// In directive argument (v-bind:[binding])
    DirectiveArg,
    /// In event handler (@click="binding")
    EventHandler,
}

/// A script binding reference in template
#[derive(Debug, Clone)]
pub struct BindingRefInfo {
    /// Span of the binding reference
    pub name_span: Span,
    /// Element ID where this reference occurs
    pub element_id: u32,
    /// Scope ID (for tracking v-for/v-slot context)
    pub scope_id: u32,
    /// Context where this binding is used
    pub context: BindingRefContext,
}

// =============================================================================
// Template Usage Collector
// =============================================================================

/// Collector for template usage information.
/// Used by the Analysis plugin during template traversal.
#[derive(Debug, Default)]
pub struct TemplateUsageCollector {
    /// Template ref attributes (ref="name")
    pub ref_attrs: Vec<TemplateRefAttrUsage>,
    /// Slot usages (v-slot directives)
    pub slot_usages: Vec<SlotUsageInfo>,
    /// Slot definitions (<slot> elements)
    pub slot_definitions: Vec<SlotDefinitionInfo>,
    /// Component usages (<MyComponent>)
    pub component_usages: Vec<ComponentUsageInfo>,
    /// Script binding references in template
    pub binding_refs: Vec<BindingRefInfo>,

    /// Loop information (v-for directives)
    pub loops: Vec<LoopInfo>,
    /// Render performance warnings detected
    pub render_warnings: Vec<RenderPatternWarning>,
    /// Aggregated template metrics
    pub metrics: TemplateMetrics,

    /// Quick lookup flags
    pub flags: FileUsageFlags,
}

impl TemplateUsageCollector {
    /// Create a new template usage collector
    pub fn new() -> Self {
        Self {
            ref_attrs: Vec::with_capacity(4),
            slot_usages: Vec::with_capacity(4),
            slot_definitions: Vec::with_capacity(2),
            component_usages: Vec::with_capacity(8),
            binding_refs: Vec::with_capacity(16),
            loops: Vec::with_capacity(4),
            render_warnings: Vec::new(),
            metrics: TemplateMetrics::default(),
            flags: FileUsageFlags::default(),
        }
    }

    /// Record a template ref attribute
    #[inline]
    pub fn record_ref_attr(&mut self, usage: TemplateRefAttrUsage) {
        self.flags.set(FileUsageFlags::HAS_TEMPLATE_REFS);
        self.ref_attrs.push(usage);
    }

    /// Record a slot usage
    #[inline]
    pub fn record_slot_usage(&mut self, usage: SlotUsageInfo) {
        self.flags.set(FileUsageFlags::HAS_SLOT_USAGE);
        self.slot_usages.push(usage);
    }

    /// Record a slot definition
    #[inline]
    pub fn record_slot_definition(&mut self, info: SlotDefinitionInfo) {
        self.flags.set(FileUsageFlags::HAS_SLOT_DEFINITIONS);
        self.slot_definitions.push(info);
    }

    /// Record a component usage
    #[inline]
    pub fn record_component_usage(&mut self, usage: ComponentUsageInfo) {
        self.flags.set(FileUsageFlags::HAS_COMPONENT_USAGE);
        self.component_usages.push(usage);
    }

    /// Record a binding reference
    #[inline]
    pub fn record_binding_ref(&mut self, info: BindingRefInfo) {
        self.binding_refs.push(info);
    }

    /// Record a loop (v-for)
    #[inline]
    pub fn record_loop(&mut self, info: LoopInfo) {
        // Update metrics (always count, even unreachable - for accurate file stats)
        self.metrics.loop_count += 1;
        if info.depth > self.metrics.max_loop_depth {
            self.metrics.max_loop_depth = info.depth;
        }

        // Skip warnings for:
        // 1. Loops in unreachable branches (v-if="false")
        // 2. Static loops (v-for="i in 5") - known iteration count, less concerning
        let skip_warnings = info.in_unreachable_branch || info.iterable_type.is_static();

        if !skip_warnings {
            // Check for warnings
            if !info.has_key {
                self.render_warnings
                    .push(RenderPatternWarning::LoopWithoutKey {
                        loop_span: info.span,
                        element_id: info.element_id,
                    });
            }

            if info.has_condition_on_same {
                self.render_warnings
                    .push(RenderPatternWarning::LoopWithConditionOnSame {
                        loop_span: info.span,
                        element_id: info.element_id,
                    });
            }

            if info.depth >= 2 {
                // Find the parent loop for nested loop warning
                // Only warn if parent is also not in unreachable branch
                if let Some(parent_id) = info.parent_loop_id {
                    if let Some(parent_loop) = self.loops.iter().find(|l| l.element_id == parent_id)
                    {
                        if !parent_loop.in_unreachable_branch {
                            self.render_warnings.push(RenderPatternWarning::NestedLoop {
                                outer_span: parent_loop.span,
                                inner_span: info.span,
                                depth: info.depth,
                            });
                        }
                    }
                }
            }
        }

        self.loops.push(info);
    }

    /// Record a render pattern warning
    #[inline]
    pub fn record_warning(&mut self, warning: RenderPatternWarning) {
        self.render_warnings.push(warning);
    }

    /// Increment element count
    #[inline]
    pub fn increment_element_count(&mut self) {
        self.metrics.element_count += 1;
    }

    /// Increment interpolation count
    #[inline]
    pub fn increment_interpolation_count(&mut self) {
        self.metrics.interpolation_count += 1;
    }

    /// Increment directive count
    #[inline]
    pub fn increment_directive_count(&mut self) {
        self.metrics.directive_count += 1;
    }

    /// Record a conditional and update chain tracking
    #[inline]
    pub fn record_conditional(&mut self, chain_length: u32) {
        self.metrics.conditional_count += 1;
        if chain_length > self.metrics.max_conditional_chain {
            self.metrics.max_conditional_chain = chain_length;
        }
    }

    /// Finalize metrics after traversal is complete
    pub fn finalize_metrics(&mut self) {
        self.metrics.component_count = self.component_usages.len() as u32;
        self.metrics.slot_definition_count = self.slot_definitions.len() as u32;
        self.metrics.slot_usage_count = self.slot_usages.len() as u32;
        self.metrics.ref_count = self.ref_attrs.len() as u32;

        // Count unique binding references (by span to dedupe)
        let mut unique_spans: std::collections::HashSet<(u32, u32)> =
            std::collections::HashSet::new();
        for binding_ref in &self.binding_refs {
            unique_spans.insert((binding_ref.name_span.start, binding_ref.name_span.end));
        }
        self.metrics.unique_binding_refs = unique_spans.len() as u32;
    }

    /// Check if any usage was collected
    pub fn is_empty(&self) -> bool {
        self.flags.bits() == 0 && self.binding_refs.is_empty()
    }

    /// Get warnings by severity
    pub fn warnings_by_severity(&self, severity: WarningSeverity) -> Vec<&RenderPatternWarning> {
        self.render_warnings
            .iter()
            .filter(|w| w.severity() == severity)
            .collect()
    }

    /// Check if there are any render warnings
    pub fn has_render_warnings(&self) -> bool {
        !self.render_warnings.is_empty()
    }

    /// Take ownership of collected data, leaving empty collector
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }
}

// =============================================================================
// Loop & Render Performance Tracking
// =============================================================================

/// Static evaluation result for a condition expression.
///
/// Used to detect unreachable code branches (v-if="false", v-else after v-if="true").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StaticConditionValue {
    /// Always true: v-if="true", v-if="1", v-if="'non-empty'"
    AlwaysTrue,
    /// Always false: v-if="false", v-if="0", v-if="''", v-if="null"
    AlwaysFalse,
    /// Cannot determine statically (depends on runtime values)
    #[default]
    Dynamic,
}

impl StaticConditionValue {
    /// Evaluate a condition expression to determine if it's statically known.
    ///
    /// Detects:
    /// - `true`, `false` literals
    /// - Numeric literals (0 is falsy, non-zero is truthy)
    /// - String literals (empty is falsy, non-empty is truthy)
    /// - `null`, `undefined` literals
    pub fn from_expression_bytes(expr: &[u8]) -> Self {
        let trimmed = expr.trim_ascii();

        // Check for boolean literals
        if trimmed == b"true" {
            return Self::AlwaysTrue;
        }
        if trimmed == b"false" {
            return Self::AlwaysFalse;
        }

        // Check for null/undefined
        if trimmed == b"null" || trimmed == b"undefined" {
            return Self::AlwaysFalse;
        }

        // Check for numeric literals
        if let Ok(s) = std::str::from_utf8(trimmed) {
            if let Ok(n) = s.parse::<f64>() {
                return if n == 0.0 {
                    Self::AlwaysFalse
                } else {
                    Self::AlwaysTrue
                };
            }
        }

        // Check for empty string literal
        if trimmed == b"''" || trimmed == b"\"\"" {
            return Self::AlwaysFalse;
        }

        // Check for non-empty string literal (starts and ends with quotes)
        if trimmed.len() >= 2 {
            let first = trimmed[0];
            let last = trimmed[trimmed.len() - 1];
            if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
                // Non-empty string literal is truthy
                return Self::AlwaysTrue;
            }
        }

        Self::Dynamic
    }

    /// Check if this branch is unreachable
    pub const fn is_unreachable(&self) -> bool {
        matches!(self, Self::AlwaysFalse)
    }

    /// Check if siblings after this are unreachable (v-else after v-if="true")
    pub const fn siblings_unreachable(&self) -> bool {
        matches!(self, Self::AlwaysTrue)
    }
}

/// Type of iterable in a v-for directive.
///
/// Distinguishes static iterables (known at compile time) from dynamic ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IterableType {
    /// Static numeric range: v-for="i in 5" or v-for="i in 10"
    /// The u32 is the count.
    StaticNumber(u32),
    /// Dynamic iterable: v-for="item in items" or v-for="item in getItems()"
    #[default]
    Dynamic,
}

impl IterableType {
    /// Parse an iterable expression to determine if it's static.
    ///
    /// Detects:
    /// - Numeric literals: `5`, `10`, `100`
    /// - Does NOT detect array literals (would need full parsing)
    pub fn from_expression_bytes(expr: &[u8]) -> Self {
        let trimmed = expr.trim_ascii();

        // Check for numeric literal
        if let Ok(s) = std::str::from_utf8(trimmed) {
            if let Ok(n) = s.parse::<u32>() {
                return Self::StaticNumber(n);
            }
        }

        Self::Dynamic
    }

    /// Check if this is a static iterable
    pub const fn is_static(&self) -> bool {
        matches!(self, Self::StaticNumber(_))
    }

    /// Get the static count if known
    pub const fn static_count(&self) -> Option<u32> {
        match self {
            Self::StaticNumber(n) => Some(*n),
            Self::Dynamic => None,
        }
    }
}

/// Information about a v-for loop in the template.
///
/// Tracks loop nesting, children composition, and potential performance issues.
#[derive(Debug, Clone)]
pub struct LoopInfo {
    /// Span of the v-for directive
    pub span: Span,
    /// Element ID where this loop is defined
    pub element_id: u32,
    /// Loop nesting depth (1 = top-level, 2 = nested in another loop, etc.)
    pub depth: u32,
    /// Parent loop's element_id if nested (None for top-level loops)
    pub parent_loop_id: Option<u32>,
    /// Whether the loop has a :key attribute
    pub has_key: bool,
    /// Whether v-if/v-else-if/v-else is on the same element (anti-pattern)
    pub has_condition_on_same: bool,
    /// Span of the iterable expression (the "items" in "item in items")
    pub iterable_span: Span,
    /// Type of iterable (static number vs dynamic)
    pub iterable_type: IterableType,
    /// Whether this loop is inside an unreachable branch (v-if="false")
    pub in_unreachable_branch: bool,
}

/// Children composition inside a loop body.
///
/// Tracked separately because we need to see all children before computing.
#[derive(Debug, Clone, Default)]
pub struct LoopChildren {
    /// Number of direct component children
    pub component_count: u32,
    /// Number of direct non-component element children
    pub element_count: u32,
    /// Script binding references inside the loop body (external deps)
    pub external_binding_refs: Vec<Span>,
    /// References to loop variables (item, index) - expected, good pattern
    pub loop_var_refs: Vec<Span>,
}

/// A potential render performance issue detected during analysis.
#[derive(Debug, Clone)]
pub enum RenderPatternWarning {
    /// v-for without :key - Vue falls back to in-place patch strategy
    /// This can cause issues with stateful elements and is slower for reordering.
    LoopWithoutKey { loop_span: Span, element_id: u32 },

    /// v-for with v-if on same element - Vue processes v-if first (Vue 3)
    /// Recommended: use <template v-for> with v-if on child, or computed filter.
    LoopWithConditionOnSame { loop_span: Span, element_id: u32 },

    /// Nested loops - O(n×m) render complexity
    /// Consider: pagination, virtualization, or flattening data structure.
    NestedLoop {
        outer_span: Span,
        inner_span: Span,
        depth: u32,
    },

    /// Loop body uses script bindings that aren't loop variables.
    /// When these external deps change, the entire list re-renders.
    /// Consider: extract to a child component with props.
    LoopWithExternalDeps {
        loop_span: Span,
        element_id: u32,
        /// Spans of external binding references inside the loop
        external_dep_spans: Vec<Span>,
    },

    /// Loop without component children - no caching benefit.
    /// If list items are complex, consider extracting to a component.
    LoopWithoutComponents {
        loop_span: Span,
        element_id: u32,
        /// How many raw elements are rendered per iteration
        element_count: u32,
    },
}

impl RenderPatternWarning {
    /// Get the span of the primary issue location
    pub fn span(&self) -> Span {
        match self {
            Self::LoopWithoutKey { loop_span, .. } => *loop_span,
            Self::LoopWithConditionOnSame { loop_span, .. } => *loop_span,
            Self::NestedLoop { inner_span, .. } => *inner_span,
            Self::LoopWithExternalDeps { loop_span, .. } => *loop_span,
            Self::LoopWithoutComponents { loop_span, .. } => *loop_span,
        }
    }

    /// Get a static message describing the issue
    pub const fn message(&self) -> &'static str {
        match self {
            Self::LoopWithoutKey { .. } => {
                "v-for without :key - may cause rendering issues and performance degradation"
            }
            Self::LoopWithConditionOnSame { .. } => {
                "v-if on same element as v-for - use <template v-for> or computed filtering"
            }
            Self::NestedLoop { .. } => {
                "Nested loops create O(n×m) render complexity - consider pagination or virtualization"
            }
            Self::LoopWithExternalDeps { .. } => {
                "Loop body depends on external bindings - extract to child component for caching"
            }
            Self::LoopWithoutComponents { .. } => {
                "Loop renders raw elements - consider component extraction for complex items"
            }
        }
    }

    /// Get the severity level
    pub const fn severity(&self) -> WarningSeverity {
        match self {
            Self::LoopWithoutKey { .. } => WarningSeverity::Warning,
            Self::LoopWithConditionOnSame { .. } => WarningSeverity::Warning,
            Self::NestedLoop { depth, .. } => {
                if *depth >= 3 {
                    WarningSeverity::Warning
                } else {
                    WarningSeverity::Info
                }
            }
            Self::LoopWithExternalDeps { .. } => WarningSeverity::Info,
            Self::LoopWithoutComponents { .. } => WarningSeverity::Hint,
        }
    }
}

/// Severity level for render pattern warnings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    /// Just a hint for potential optimization
    Hint,
    /// Informational - might be intentional
    Info,
    /// Warning - likely a performance issue
    Warning,
}

// =============================================================================
// Template Metrics
// =============================================================================

/// Aggregated metrics about template complexity and structure.
///
/// Useful for complexity scoring and quick file-level queries.
#[derive(Debug, Clone, Default)]
pub struct TemplateMetrics {
    /// Total element count (including components)
    pub element_count: u32,
    /// Component usage count
    pub component_count: u32,
    /// Total v-for loop count
    pub loop_count: u32,
    /// Maximum loop nesting depth (0 if no loops)
    pub max_loop_depth: u32,
    /// Total conditional directive count (v-if + v-else-if + v-else + v-show)
    pub conditional_count: u32,
    /// Maximum v-if/else-if/else chain length
    pub max_conditional_chain: u32,
    /// Total interpolation count ({{ }})
    pub interpolation_count: u32,
    /// Total directive count (v-*, :*, @*)
    pub directive_count: u32,
    /// Slot definition count
    pub slot_definition_count: u32,
    /// Slot usage count
    pub slot_usage_count: u32,
    /// Template ref count
    pub ref_count: u32,
    /// Unique script bindings referenced in template
    pub unique_binding_refs: u32,
}

impl TemplateMetrics {
    /// Compute a simple complexity score.
    ///
    /// Higher score = more complex template.
    /// This is a heuristic - adjust weights based on your needs.
    pub fn complexity_score(&self) -> f32 {
        let base = self.element_count as f32 * 0.5
            + self.component_count as f32 * 1.0
            + self.interpolation_count as f32 * 0.3
            + self.directive_count as f32 * 0.5;

        let loop_penalty =
            self.loop_count as f32 * 2.0 + (self.max_loop_depth.saturating_sub(1)) as f32 * 5.0; // Nested loops are expensive

        let conditional_penalty = self.conditional_count as f32 * 0.5
            + (self.max_conditional_chain.saturating_sub(2)) as f32 * 1.0; // Long chains

        let slot_complexity =
            self.slot_definition_count as f32 * 1.5 + self.slot_usage_count as f32 * 1.0;

        base + loop_penalty + conditional_penalty + slot_complexity
    }
}

// =============================================================================
// Condition Likelihood Heuristics
// =============================================================================

/// Heuristic-based likelihood estimation for conditions.
///
/// This is speculative and based on common naming patterns.
/// Cannot determine actual runtime likelihood without profiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionLikelihood {
    /// Likely to be true most of the time (e.g., "isVisible", "hasData")
    LikelyTrue,
    /// Likely to be false most of the time (e.g., "isLoading", "error", "isAdmin")
    LikelyFalse,
    /// No heuristic available
    Unknown,
}

impl ConditionLikelihood {
    /// Estimate likelihood from a condition binding name.
    ///
    /// This uses common naming conventions as heuristics:
    /// - "loading", "pending", "fetching" → usually false (data is usually loaded)
    /// - "error", "failed", "invalid" → usually false (errors are exceptional)
    /// - "isAdmin", "isModerator", "hasPermission" → usually false (most users aren't admins)
    /// - "isVisible", "isActive", "isEnabled" → usually true (default states)
    pub fn from_binding_name(name: &str) -> Self {
        let lower = name.to_lowercase();

        // Patterns that are typically false (exceptional states)
        const USUALLY_FALSE: &[&str] = &[
            "loading",
            "isloading",
            "pending",
            "ispending",
            "fetching",
            "isfetching",
            "error",
            "haserror",
            "iserror",
            "failed",
            "hasfailed",
            "invalid",
            "isinvalid",
            "admin",
            "isadmin",
            "moderator",
            "ismoderator",
            "superuser",
            "issuperuser",
            "debug",
            "isdebug",
            "dev",
            "isdev",
            "empty",
            "isempty",
            "disabled",
            "isdisabled",
        ];

        // Patterns that are typically true (default states)
        const USUALLY_TRUE: &[&str] = &[
            "visible",
            "isvisible",
            "active",
            "isactive",
            "enabled",
            "isenabled",
            "ready",
            "isready",
            "loaded",
            "isloaded",
            "valid",
            "isvalid",
            "authenticated",
            "isauthenticated",
            "loggedin",
            "isloggedin",
        ];

        for pattern in USUALLY_FALSE {
            if lower.contains(pattern) {
                return Self::LikelyFalse;
            }
        }

        for pattern in USUALLY_TRUE {
            if lower.contains(pattern) {
                return Self::LikelyTrue;
            }
        }

        Self::Unknown
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_provide_inject() {
        assert_eq!(detect_vue_api_call(b"provide"), Some(VueApiKind::Provide));
        assert_eq!(detect_vue_api_call(b"inject"), Some(VueApiKind::Inject));
    }

    #[test]
    fn test_detect_reactivity() {
        assert_eq!(detect_vue_api_call(b"ref"), Some(VueApiKind::Ref));
        assert_eq!(detect_vue_api_call(b"reactive"), Some(VueApiKind::Reactive));
        assert_eq!(detect_vue_api_call(b"computed"), Some(VueApiKind::Computed));
        assert_eq!(
            detect_vue_api_call(b"shallowRef"),
            Some(VueApiKind::ShallowRef)
        );
        assert_eq!(
            detect_vue_api_call(b"shallowReactive"),
            Some(VueApiKind::ShallowReactive)
        );
    }

    #[test]
    fn test_detect_watchers() {
        assert_eq!(detect_vue_api_call(b"watch"), Some(VueApiKind::Watch));
        assert_eq!(
            detect_vue_api_call(b"watchEffect"),
            Some(VueApiKind::WatchEffect)
        );
        assert_eq!(
            detect_vue_api_call(b"watchPostEffect"),
            Some(VueApiKind::WatchPostEffect)
        );
        assert_eq!(
            detect_vue_api_call(b"watchSyncEffect"),
            Some(VueApiKind::WatchSyncEffect)
        );
    }

    #[test]
    fn test_detect_lifecycle_hooks() {
        assert_eq!(
            detect_vue_api_call(b"onMounted"),
            Some(VueApiKind::OnMounted)
        );
        assert_eq!(
            detect_vue_api_call(b"onUnmounted"),
            Some(VueApiKind::OnUnmounted)
        );
        assert_eq!(
            detect_vue_api_call(b"onBeforeMount"),
            Some(VueApiKind::OnBeforeMount)
        );
        assert_eq!(
            detect_vue_api_call(b"onBeforeUnmount"),
            Some(VueApiKind::OnBeforeUnmount)
        );
        assert_eq!(
            detect_vue_api_call(b"onUpdated"),
            Some(VueApiKind::OnUpdated)
        );
        assert_eq!(
            detect_vue_api_call(b"onBeforeUpdate"),
            Some(VueApiKind::OnBeforeUpdate)
        );
        assert_eq!(
            detect_vue_api_call(b"onErrorCaptured"),
            Some(VueApiKind::OnErrorCaptured)
        );
        assert_eq!(
            detect_vue_api_call(b"onActivated"),
            Some(VueApiKind::OnActivated)
        );
        assert_eq!(
            detect_vue_api_call(b"onDeactivated"),
            Some(VueApiKind::OnDeactivated)
        );
        assert_eq!(
            detect_vue_api_call(b"onRenderTracked"),
            Some(VueApiKind::OnRenderTracked)
        );
        assert_eq!(
            detect_vue_api_call(b"onRenderTriggered"),
            Some(VueApiKind::OnRenderTriggered)
        );
        assert_eq!(
            detect_vue_api_call(b"onServerPrefetch"),
            Some(VueApiKind::OnServerPrefetch)
        );
    }

    #[test]
    fn test_detect_template_utils() {
        assert_eq!(detect_vue_api_call(b"useSlots"), Some(VueApiKind::UseSlots));
        assert_eq!(detect_vue_api_call(b"useAttrs"), Some(VueApiKind::UseAttrs));
        assert_eq!(
            detect_vue_api_call(b"useTemplateRef"),
            Some(VueApiKind::UseTemplateRef)
        );
    }

    #[test]
    fn test_non_vue_apis() {
        // Too short
        assert_eq!(detect_vue_api_call(b"re"), None);
        // Too long
        assert_eq!(detect_vue_api_call(b"onRenderTriggeredExtra"), None);
        // Not Vue API
        assert_eq!(detect_vue_api_call(b"console"), None);
        assert_eq!(detect_vue_api_call(b"defineProps"), None); // This is a macro, not API
        assert_eq!(detect_vue_api_call(b"useState"), None); // React, not Vue
    }

    #[test]
    fn test_api_categories() {
        assert_eq!(
            VueApiKind::Provide.category(),
            VueApiCategory::DependencyInjection
        );
        assert_eq!(VueApiKind::Ref.category(), VueApiCategory::Reactivity);
        assert_eq!(VueApiKind::Watch.category(), VueApiCategory::Watchers);
        assert_eq!(VueApiKind::OnMounted.category(), VueApiCategory::Lifecycle);
        assert_eq!(
            VueApiKind::UseSlots.category(),
            VueApiCategory::TemplateUtils
        );
        assert_eq!(
            VueApiKind::GetCurrentInstance.category(),
            VueApiCategory::InstanceAccess
        );
    }

    #[test]
    fn test_file_usage_flags() {
        let mut flags = FileUsageFlags::default();
        assert!(!flags.has(FileUsageFlags::HAS_PROVIDE));

        flags.set(FileUsageFlags::HAS_PROVIDE);
        assert!(flags.has(FileUsageFlags::HAS_PROVIDE));
        assert!(!flags.has(FileUsageFlags::HAS_INJECT));

        flags.set(FileUsageFlags::HAS_INJECT);
        assert!(flags.has(FileUsageFlags::HAS_PROVIDE));
        assert!(flags.has(FileUsageFlags::HAS_INJECT));
    }

    #[test]
    fn test_detect_get_current_instance() {
        assert_eq!(
            detect_vue_api_call(b"getCurrentInstance"),
            Some(VueApiKind::GetCurrentInstance)
        );
        // Variations should not match
        assert_eq!(detect_vue_api_call(b"getcurrentinstance"), None);
        assert_eq!(detect_vue_api_call(b"GetCurrentInstance"), None);
    }

    #[test]
    fn test_get_current_instance_category() {
        assert_eq!(
            VueApiKind::GetCurrentInstance.category(),
            VueApiCategory::InstanceAccess
        );
    }

    #[test]
    fn test_get_current_instance_requires_sync_context() {
        assert!(VueApiKind::GetCurrentInstance.requires_sync_context());
        assert!(!VueApiKind::Ref.requires_sync_context());
        assert!(!VueApiKind::OnMounted.requires_sync_context());
    }

    #[test]
    fn test_call_site_context_safety() {
        // Safe contexts
        assert!(CallSiteContext::BeforeAwait.is_safe());
        assert!(CallSiteContext::InLifecycleCallback.is_safe());
        assert!(CallSiteContext::InReactiveCallback.is_safe());

        // Potentially unsafe contexts
        assert!(CallSiteContext::AfterAwait.is_potentially_unsafe());
        assert!(CallSiteContext::InPromiseCallback.is_potentially_unsafe());

        // Definitely unsafe contexts
        assert!(CallSiteContext::InTimerCallback.is_unsafe());

        // Unknown is neither safe nor unsafe
        assert!(!CallSiteContext::Unknown.is_safe());
        assert!(!CallSiteContext::Unknown.is_unsafe());
    }

    #[test]
    fn test_sync_context_usage() {
        let safe_usage = SyncContextUsage {
            span: Span::new(0, 20),
            kind: VueApiKind::GetCurrentInstance,
            context: CallSiteContext::BeforeAwait,
            binding_span: Some(Span::new(6, 14)),
            preceding_await_span: None,
        };
        assert!(safe_usage.is_safe());
        assert!(!safe_usage.is_potentially_unsafe());

        let unsafe_usage = SyncContextUsage {
            span: Span::new(50, 70),
            kind: VueApiKind::GetCurrentInstance,
            context: CallSiteContext::AfterAwait,
            binding_span: Some(Span::new(56, 64)),
            preceding_await_span: Some(Span::new(0, 25)),
        };
        assert!(!unsafe_usage.is_safe());
        assert!(unsafe_usage.is_potentially_unsafe());
    }

    #[test]
    fn test_usage_collector_await_tracking() {
        let source = b"const x = 1;";
        let mut collector = UsageCollector::new(source);

        // Initially no await
        assert!(!collector.has_await_before(0));
        assert!(!collector.has_await_before(100));
        assert!(collector.first_await_span().is_none());

        // Record an await at position 10-25
        collector.record_await(Span::new(10, 25));

        // Now positions after 25 should be after await
        assert!(!collector.has_await_before(0)); // Before await
        assert!(!collector.has_await_before(10)); // At await start
        assert!(!collector.has_await_before(25)); // At await end
        assert!(collector.has_await_before(26)); // After await
        assert!(collector.has_await_before(100)); // Well after await

        // First await span should be recorded
        assert_eq!(collector.first_await_span(), Some(Span::new(10, 25)));

        // Recording another await shouldn't change the first
        collector.record_await(Span::new(50, 60));
        assert_eq!(collector.first_await_span(), Some(Span::new(10, 25)));
    }

    #[test]
    fn test_usage_collector_sync_context_recording() {
        let source = b"const instance = getCurrentInstance();";
        let mut collector = UsageCollector::new(source);

        // Safe usage
        let safe = SyncContextUsage {
            span: Span::new(17, 37),
            kind: VueApiKind::GetCurrentInstance,
            context: CallSiteContext::BeforeAwait,
            binding_span: Some(Span::new(6, 14)),
            preceding_await_span: None,
        };
        collector.record_sync_context_usage(safe);

        assert!(collector.flags.has(FileUsageFlags::HAS_SYNC_CONTEXT_USAGE));
        assert!(!collector.flags.has(FileUsageFlags::HAS_UNSAFE_SYNC_CONTEXT));
        assert!(!collector.has_unsafe_sync_context());
        assert_eq!(collector.sync_context_usages.len(), 1);

        // Unsafe usage
        let unsafe_usage = SyncContextUsage {
            span: Span::new(100, 120),
            kind: VueApiKind::GetCurrentInstance,
            context: CallSiteContext::AfterAwait,
            binding_span: None,
            preceding_await_span: Some(Span::new(50, 60)),
        };
        collector.record_sync_context_usage(unsafe_usage);

        assert!(collector.flags.has(FileUsageFlags::HAS_UNSAFE_SYNC_CONTEXT));
        assert!(collector.has_unsafe_sync_context());
        assert_eq!(collector.sync_context_usages.len(), 2);

        // Iterator should return only unsafe
        let unsafe_count = collector.unsafe_sync_context_usages().count();
        assert_eq!(unsafe_count, 1);
    }

    // ==================== Loop Tracking Tests ====================

    #[test]
    fn test_loop_info_recording() {
        let mut collector = TemplateUsageCollector::new();

        // Record a top-level loop without key
        collector.record_loop(LoopInfo {
            span: Span::new(10, 50),
            element_id: 1,
            depth: 1,
            parent_loop_id: None,
            has_key: false,
            has_condition_on_same: false,
            iterable_span: Span::new(30, 35),
            iterable_type: IterableType::Dynamic,
            in_unreachable_branch: false,
        });

        assert_eq!(collector.loops.len(), 1);
        assert_eq!(collector.metrics.loop_count, 1);
        assert_eq!(collector.metrics.max_loop_depth, 1);

        // Should have a warning for missing key
        assert!(collector.has_render_warnings());
        assert_eq!(collector.render_warnings.len(), 1);
        assert!(matches!(
            collector.render_warnings[0],
            RenderPatternWarning::LoopWithoutKey { .. }
        ));
    }

    #[test]
    fn test_loop_with_key_no_warning() {
        let mut collector = TemplateUsageCollector::new();

        collector.record_loop(LoopInfo {
            span: Span::new(10, 50),
            element_id: 1,
            depth: 1,
            parent_loop_id: None,
            has_key: true, // Has key
            has_condition_on_same: false,
            iterable_span: Span::new(30, 35),
            iterable_type: IterableType::Dynamic,
            in_unreachable_branch: false,
        });

        // Should not have a warning for missing key
        assert!(!collector.has_render_warnings());
    }

    #[test]
    fn test_loop_with_condition_warning() {
        let mut collector = TemplateUsageCollector::new();

        collector.record_loop(LoopInfo {
            span: Span::new(10, 50),
            element_id: 1,
            depth: 1,
            parent_loop_id: None,
            has_key: true,
            has_condition_on_same: true, // v-if on same element
            iterable_span: Span::new(30, 35),
            iterable_type: IterableType::Dynamic,
            in_unreachable_branch: false,
        });

        assert!(collector.has_render_warnings());
        assert!(matches!(
            collector.render_warnings[0],
            RenderPatternWarning::LoopWithConditionOnSame { .. }
        ));
    }

    #[test]
    fn test_nested_loop_tracking() {
        let mut collector = TemplateUsageCollector::new();

        // Record outer loop
        collector.record_loop(LoopInfo {
            span: Span::new(10, 100),
            element_id: 1,
            depth: 1,
            parent_loop_id: None,
            has_key: true,
            has_condition_on_same: false,
            iterable_span: Span::new(30, 35),
            iterable_type: IterableType::Dynamic,
            in_unreachable_branch: false,
        });

        // Record inner loop (nested)
        collector.record_loop(LoopInfo {
            span: Span::new(40, 80),
            element_id: 2,
            depth: 2,
            parent_loop_id: Some(1),
            has_key: true,
            has_condition_on_same: false,
            iterable_span: Span::new(50, 55),
            iterable_type: IterableType::Dynamic,
            in_unreachable_branch: false,
        });

        assert_eq!(collector.loops.len(), 2);
        assert_eq!(collector.metrics.loop_count, 2);
        assert_eq!(collector.metrics.max_loop_depth, 2);

        // Should have nested loop warning
        let nested_warnings: Vec<_> = collector
            .render_warnings
            .iter()
            .filter(|w| matches!(w, RenderPatternWarning::NestedLoop { .. }))
            .collect();
        assert_eq!(nested_warnings.len(), 1);
    }

    #[test]
    fn test_template_metrics_complexity_score() {
        let metrics = TemplateMetrics {
            element_count: 10,
            component_count: 3,
            loop_count: 2,
            max_loop_depth: 2, // Nested loop
            conditional_count: 4,
            max_conditional_chain: 3,
            interpolation_count: 5,
            directive_count: 8,
            slot_definition_count: 1,
            slot_usage_count: 2,
            ref_count: 2,
            unique_binding_refs: 6,
        };

        let score = metrics.complexity_score();
        assert!(score > 0.0);

        // Nested loops should increase complexity
        let simple_metrics = TemplateMetrics {
            element_count: 10,
            component_count: 3,
            loop_count: 1,
            max_loop_depth: 1, // No nesting
            ..Default::default()
        };

        assert!(metrics.complexity_score() > simple_metrics.complexity_score());
    }

    #[test]
    fn test_render_pattern_warning_severity() {
        let no_key = RenderPatternWarning::LoopWithoutKey {
            loop_span: Span::new(0, 10),
            element_id: 1,
        };
        assert_eq!(no_key.severity(), WarningSeverity::Warning);

        let nested = RenderPatternWarning::NestedLoop {
            outer_span: Span::new(0, 100),
            inner_span: Span::new(20, 80),
            depth: 2,
        };
        assert_eq!(nested.severity(), WarningSeverity::Info);

        // Deeply nested should be warning
        let deep_nested = RenderPatternWarning::NestedLoop {
            outer_span: Span::new(0, 100),
            inner_span: Span::new(20, 80),
            depth: 3,
        };
        assert_eq!(deep_nested.severity(), WarningSeverity::Warning);
    }

    // ==================== Condition Likelihood Tests ====================

    #[test]
    fn test_condition_likelihood_usually_false() {
        assert_eq!(
            ConditionLikelihood::from_binding_name("isLoading"),
            ConditionLikelihood::LikelyFalse
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("hasError"),
            ConditionLikelihood::LikelyFalse
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("isAdmin"),
            ConditionLikelihood::LikelyFalse
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("isEmpty"),
            ConditionLikelihood::LikelyFalse
        );
    }

    #[test]
    fn test_condition_likelihood_usually_true() {
        assert_eq!(
            ConditionLikelihood::from_binding_name("isVisible"),
            ConditionLikelihood::LikelyTrue
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("isActive"),
            ConditionLikelihood::LikelyTrue
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("isReady"),
            ConditionLikelihood::LikelyTrue
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("isAuthenticated"),
            ConditionLikelihood::LikelyTrue
        );
    }

    #[test]
    fn test_condition_likelihood_unknown() {
        assert_eq!(
            ConditionLikelihood::from_binding_name("showModal"),
            ConditionLikelihood::Unknown
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("count"),
            ConditionLikelihood::Unknown
        );
        assert_eq!(
            ConditionLikelihood::from_binding_name("someFlag"),
            ConditionLikelihood::Unknown
        );
    }

    #[test]
    fn test_template_usage_collector_finalize() {
        let mut collector = TemplateUsageCollector::new();

        // Add some data
        collector.record_component_usage(ComponentUsageInfo {
            span: Span::new(0, 10),
            name_span: Span::new(1, 9),
            element_id: 1,
            is_dynamic: false,
        });

        collector.record_binding_ref(BindingRefInfo {
            name_span: Span::new(10, 15),
            element_id: 1,
            scope_id: 0,
            context: BindingRefContext::Interpolation,
        });

        collector.record_binding_ref(BindingRefInfo {
            name_span: Span::new(10, 15), // Same span (should be deduped)
            element_id: 2,
            scope_id: 0,
            context: BindingRefContext::DirectiveValue,
        });

        collector.finalize_metrics();

        assert_eq!(collector.metrics.component_count, 1);
        assert_eq!(collector.metrics.unique_binding_refs, 1); // Deduped
    }
}
