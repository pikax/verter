//! Types for template code generation.

use crate::common::Span;
use crate::syntax::types::{OxcVConditionType, SyntaxTagType};
use rustc_hash::{FxHashMap, FxHashSet};

// ============================================================================
// Binding Metadata
// ============================================================================

/// Whitespace handling strategy for template compilation.
/// Matches Vue's official compiler options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhitespaceMode {
    /// Condense mode (default): collapse adjacent whitespace to a single space,
    /// remove whitespace-only text nodes that contain newlines.
    #[default]
    Condense,
    /// Preserve mode: keep all whitespace as-is.
    Preserve,
}

/// Classification of a binding for correct accessor prefix in template codegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingType {
    /// const, let, function, import → Dev: `$setup.x` | Prod: `x` (closure)
    Setup,
    /// const x = ref()/computed()/shallowRef() → Dev: `$setup.x` | Prod: `x.value` (closure)
    SetupRef,
    /// defineProps / defineModel prop → Dev: `$props.x` (render param) | Prod: `__props.x` (setup param)
    Props,
}

/// Zero-allocation binding metadata. Stores `(Span, BindingType)` pairs where
/// each `Span` references identifier bytes in the original SFC source.
#[derive(Debug, Default, Clone)]
pub struct BindingMetadata {
    pub entries: Vec<(Span, BindingType)>,
}

impl BindingMetadata {
    /// Look up binding type by comparing identifier bytes against source spans.
    pub fn get(&self, ident: &[u8], source: &[u8]) -> Option<BindingType> {
        self.entries
            .iter()
            .find(|(span, _)| &source[span.start as usize..span.end as usize] == ident)
            .map(|(_, bt)| *bt)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Resolve the correct accessor prefix for an identifier.
/// Returns a static `&str` — no allocation.
///
/// The `is_inline` flag controls the prefix style:
/// - **Inline mode** (`true`): Template is inlined inside setup() closure.
///   Props use `__props.`, setup bindings use bare identifier (no prefix).
/// - **Standalone mode** (`false`): Template is a separate `export function render(...)`.
///   Props use `$props.`, setup bindings use `$setup.`.
///
/// Note: `is_inline` is NOT the same as `is_production`. Production Vite builds
/// use standalone render functions (is_inline=false), while monolithic `generate()`
/// in prod mode uses inline (is_inline=true).
pub fn resolve_binding_prefix(
    ident: &[u8],
    metadata: &BindingMetadata,
    source: &[u8],
    is_inline: bool,
) -> &'static str {
    match metadata.get(ident, source) {
        Some(BindingType::Props) => {
            if is_inline {
                "__props."
            } else {
                "$props."
            }
        }
        Some(BindingType::Setup | BindingType::SetupRef) => {
            if is_inline {
                ""
            } else {
                "$setup."
            }
        }
        None => "_ctx.",
    }
}

/// Resolve the correct accessor suffix for an identifier.
/// Returns `.value` for `SetupRef` in inline mode, empty string otherwise.
///
/// In inline mode, ref-type bindings (created by `ref()`, `computed()`, etc.)
/// need `.value` appended to access the underlying value.
pub fn resolve_binding_suffix(
    ident: &[u8],
    metadata: &BindingMetadata,
    source: &[u8],
    is_inline: bool,
) -> &'static str {
    if !is_inline {
        return "";
    }
    match metadata.get(ident, source) {
        Some(BindingType::SetupRef) => ".value",
        _ => "",
    }
}

// ============================================================================
// Streaming Codegen State
// ============================================================================

/// Main state for streaming template codegen.
/// Minimal state - emit code as events arrive.
#[derive(Debug, Default)]
pub struct TemplateCodegenState {
    /// Pending close actions keyed by scope_id.
    /// When AnalysedCloseScopes fires, we look up and emit these.
    pub scope_close_actions: FxHashMap<u32, CloseAction>,

    /// Current element being built (props accumulate here until OpenTagEnd).
    pub current_element: Option<CurrentElement>,

    /// Active conditional chain tracking.
    pub conditional_chain: Option<ConditionalChain>,

    /// Required Vue runtime helpers (for import generation).
    pub helpers: HelperFlags,

    /// Depth of active v-slot template scopes (used to avoid default slot emission inside slots).
    pub active_vslot_depth: usize,

    /// Custom directives that need to be resolved at render function start.
    /// Contains directive names (e.g., "focus", "tooltip").
    pub resolved_directives: Vec<String>,

    /// Components that need to be resolved at render function start.
    /// Contains component names (e.g., "CustomInput", "MyButton").
    pub resolved_components: Vec<String>,

    /// Elements that have custom directives (for wrapping at close time).
    /// Maps element_id to the list of custom directives.
    pub elements_with_directives: FxHashMap<u32, Vec<CustomDirectiveEntry>>,

    /// Elements that have v-once directive (for cache wrapper at close time).
    /// Maps element_id to the cache index used for this element.
    pub elements_with_vonce: FxHashMap<u32, usize>,

    /// Elements that have v-model directive (for directive wrapper at close time).
    /// Maps element_id to (VModelInfo, tag_name) tuple.
    pub elements_with_vmodel: FxHashMap<u32, (VModelInfo, String)>,

    /// Elements that have v-show directive (for directive wrapper at close time).
    /// Maps element_id to the v-show expression span.
    pub elements_with_vshow: FxHashMap<u32, Span>,

    /// Whether we've emitted the render function opening.
    pub render_started: bool,

    /// Count of root-level elements (for Fragment wrapping).
    pub root_element_count: usize,

    /// Whether the root contains non-element children (comments/text).
    pub root_has_non_element_child: bool,

    /// Whether a root child has been emitted (for comma insertion).
    pub root_child_emitted: bool,

    /// Generated code for each root element (for Fragment wrapping).
    pub root_elements: Vec<RootElementCode>,

    /// Whether we're currently in the first pass counting roots.
    /// When true, we collect root element code instead of emitting directly.
    pub collecting_roots: bool,

    /// Set of element IDs that are root elements (for proper closing).
    pub root_element_ids: rustc_hash::FxHashSet<u32>,

    /// Source span of the first root element's opening tag (for multi-root patching).
    /// Stored as (source_start, source_end).
    pub first_root_source_span: Option<(u32, u32)>,

    /// Element ID of the first root element (for updating is_block_root on close).
    pub first_root_element_id: Option<u32>,

    /// Tag name of the first root element (for multi-root patching).
    pub first_root_tag_name: Option<String>,

    /// Complete opening code for the first root element (for multi-root patching).
    /// This includes the function call and props, e.g., `(_openBlock(), _createElementBlock("div", {props...}`
    pub first_root_opening_code: Option<String>,

    /// Whether the first root element is self-closing (no children possible).
    pub first_root_is_self_closing: bool,

    /// Close tag source span of the first root element (for multi-root patching).
    /// Stored as (source_start, source_end).
    pub first_root_close_span: Option<(u32, u32)>,

    /// The closing code that was generated for the first root element.
    pub first_root_close_code: Option<String>,

    /// Set of element IDs that have interpolation children (for TEXT patch flag).
    pub elements_with_interpolation: rustc_hash::FxHashSet<u32>,

    /// Current conditional branch index (for key generation).
    /// Reset when a new v-if chain starts, incremented for each branch.
    pub conditional_branch_index: usize,

    /// Next conditional branch key per parent depth.
    /// Vue assigns keys sequentially across sibling conditional chains at the same depth.
    pub conditional_next_key_by_depth: FxHashMap<usize, usize>,

    /// Whether we're currently in a conditional chain.
    pub in_conditional_chain: bool,

    /// Depth at which the conditional chain started.
    /// Used to only close the chain at the same depth level.
    pub conditional_chain_depth: usize,

    /// Stack of saved conditional chain states for nested v-if support.
    /// When an inner v-if starts, the outer chain state is pushed here.
    /// When the inner chain closes, the outer state is restored.
    pub conditional_chain_stack: Vec<(bool, usize, usize)>, // (in_chain, depth, branch_index)

    /// Template root span for final wrapper.
    pub template_span: Option<Span>,

    /// Depth tracking for nested elements.
    pub depth: usize,

    /// Stack tracking component children state (for slots vs array children).
    pub component_stack: Vec<ComponentChildrenState>,

    /// Positions of elements closed via v-slot (to skip their CloseTag).
    pub vslot_closed_positions: rustc_hash::FxHashSet<u32>,

    /// Stack tracking slot element state (for proper closing).
    pub slot_stack: Vec<SlotElementState>,

    /// Stack tracking whether first child has been emitted at each depth.
    /// Used to add commas between sibling elements.
    pub first_child_at_depth: Vec<bool>,

    /// Patch flags for each element by element_id.
    /// Used to emit patch flags when element closes.
    pub element_patch_flags: FxHashMap<u32, u32>,

    /// Dynamic prop names for each element by element_id.
    /// Used to emit dynamic props array when element closes.
    pub element_dynamic_props: FxHashMap<u32, Vec<String>>,

    /// Stack of element_ids for native elements.
    /// Used to look up patch flags when closing elements.
    pub element_id_stack: Vec<u32>,

    /// Hoisted static nodes (const _hoisted_N = ...).
    pub hoisted_nodes: Vec<HoistedNode>,

    /// Counter for hoisted node naming (_hoisted_1, _hoisted_2, etc.).
    pub hoist_counter: usize,

    /// Counter for cache indices (_cache[0], _cache[1], etc.).
    pub cache_index: usize,

    /// Static asset imports (e.g., `import _imports_0 from "/image.svg?import"`).
    /// Vec of (import_name, asset_path) pairs.
    pub asset_imports: Vec<(String, String)>,

    /// Counter for asset import naming (_imports_0, _imports_1, etc.).
    pub asset_import_counter: usize,

    /// Stack tracking whether each element is static.
    /// Used to determine if children can be hoisted.
    pub static_element_stack: Vec<bool>,

    /// Child count per element (for single child optimization).
    /// Key is element_id, value is number of children.
    pub element_child_count: FxHashMap<u32, usize>,

    /// Position where children array bracket should be inserted (for deferred emission).
    /// Key is element_id, value is source position after props.
    pub element_children_insert_pos: FxHashMap<u32, u32>,

    /// Single child content per element (for single child optimization).
    /// If element has exactly one text/interpolation child, store its content here.
    /// Key is element_id, value is the child content string.
    pub element_single_child: FxHashMap<u32, SingleChildInfo>,

    /// Tracks whether the children array bracket "[" has been emitted for each element.
    /// Key is element_id, value is true if "[" was emitted.
    pub element_array_opened: FxHashMap<u32, bool>,

    /// Tracks whether each element is a block root (needs _openBlock/_createElementBlock).
    /// Key is element_id, value is true if block root.
    pub element_is_block_root: FxHashMap<u32, bool>,

    /// Tracks which elements are cached and their cache index.
    /// Key is element_id, value is cache index.
    pub element_cache_index: FxHashMap<u32, usize>,
    /// Tracks which cached elements need a trailing comma (multi-root cache).
    pub element_cache_needs_comma: FxHashSet<u32>,

    /// Tracks whether the last child of an element was text/interpolation content.
    /// Used to determine if we need to add `+` for concatenation.
    /// Key is element_id, value is true if last child was text/interpolation.
    pub last_child_is_text_content: FxHashMap<u32, bool>,

    /// Maps element_id to scope_id for v-for elements.
    /// Used to find and process v-for close actions for self-closing elements.
    pub element_vfor_scope: FxHashMap<u32, u32>,

    /// Position where the script block ends (after `</script>` replacement).
    /// Used to ensure template content is placed AFTER the component definition.
    pub script_end_position: Option<u32>,

    /// Stack of v-for local variable names, accumulated from nested v-for loops.
    /// Each entry contains the locals from one v-for level.
    /// Used to prevent adding _ctx. prefix to loop variables in nested scopes.
    pub vfor_locals_stack: Vec<Vec<String>>,

    /// Stack of v-slot local variable names, accumulated from nested scoped slots.
    /// Each entry contains the destructured param names from one slot level.
    /// Used to prevent adding _ctx. prefix to slot-scoped variables.
    pub vslot_locals_stack: Vec<Vec<String>>,

    /// Elements that have v-for (for popping vfor_locals_stack on close).
    /// Set of element_ids that have v-for.
    pub elements_with_vfor: rustc_hash::FxHashSet<u32>,

    /// Production mode - affects comment node text and other prod optimizations.
    pub is_production: bool,

    /// Inline mode - when true, the template is inlined into setup() closure.
    /// This affects identifier prefixing:
    ///   - Inline mode (true):  Props → `__props.x`, Setup → bare `x` or `x.value`
    ///   - Standalone mode (false): Props → `$props.x`, Setup → `$setup.x`
    ///
    /// For `generate_for_vite`, this is always false (standalone render function).
    /// For monolithic `generate()` in prod mode, this is true (inline in setup).
    pub is_inline_mode: bool,

    /// Closing parenthesis needed after the component object in inline mode.
    /// Set to ")" when TypeScript (to close _defineComponent()), empty string otherwise.
    /// Used by finalize_template() to properly close the component definition.
    pub inline_closing_paren: String,

    /// Whitespace handling mode (condense or preserve).
    pub whitespace: WhitespaceMode,

    /// Binding metadata from `<script setup>` for correct accessor prefixes.
    /// Maps identifier spans to binding types (Setup vs Props).
    pub binding_metadata: BindingMetadata,

    // ============================================================================
    // Scoped Styles State
    // ============================================================================
    /// Scope ID for scoped styles - 8 bytes (e.g., b"a4f2eed6").
    /// Use scope_id.is_some() to check if scoped styles exist.
    /// When set, elements will get data-v-{id} attribute.
    pub scope_id: Option<[u8; 8]>,

    /// v-bind() expressions from CSS for inline style injection.
    /// These are collected from <style scoped> blocks and applied
    /// as inline styles on the root element.
    pub css_v_bind_expressions: Vec<crate::syntax::types::CssVBindExpression>,

    /// Transformed CSS content from scoped styles.
    /// Stored here for later export as __css__.
    pub transformed_css: Option<Vec<u8>>,

    // ============================================================================
    // V-Slot Text VNode Concatenation State
    // ============================================================================
    /// Stack of saved conditional chain states at v-slot boundaries.
    /// Separate from `conditional_chain_stack` (used for nested v-if) to avoid
    /// v-else accidentally popping a v-slot save entry.
    pub vslot_conditional_chain_stack: Vec<(bool, usize, usize)>, // (in_chain, depth, branch_index)

    /// Whether a `_createTextVNode(` is currently open for v-slot text concatenation.
    /// When true, subsequent text/interpolation in the v-slot appends with ` + `.
    pub vslot_text_vnode_open: bool,

    /// End position of the last text/interpolation in the open v-slot text vnode.
    /// Used for appending the closing `)` when flushing.
    pub vslot_text_vnode_last_end: u32,

    /// Whether the open v-slot text vnode contains any interpolation.
    /// Determines whether to include the TEXT patch flag (1) on close.
    pub vslot_text_vnode_has_interp: bool,

    // ============================================================================
    // CSS Modules State
    // ============================================================================
    /// CSS modules class mappings.
    /// Maps module name (e.g., "$style") to class mappings (original → hashed).
    pub css_modules: Vec<CssModuleEntry>,
}

/// Entry for a CSS module
#[derive(Debug, Clone, Default)]
pub struct CssModuleEntry {
    /// Module name (e.g., "$style" or custom name)
    pub name: String,
    /// Class name mappings (original → hashed)
    pub classes: Vec<(String, String)>,
    /// Transformed CSS content
    pub css: Vec<u8>,
}

/// Information about a single child for optimization.
#[derive(Debug, Clone)]
pub struct SingleChildInfo {
    /// The generated code for the single child.
    pub content: String,
    /// Whether this is an interpolation (vs static text).
    pub is_interpolation: bool,
    /// Original source span start (for array wrapping when element child arrives).
    pub start: u32,
    /// Original source span end.
    pub end: u32,
}

/// A hoisted static node.
#[derive(Debug, Clone)]
pub struct HoistedNode {
    /// Variable name (e.g., "_hoisted_1").
    pub name: String,
    /// The static props/node code.
    pub code: String,
}

/// Generated code for a root element (used for Fragment wrapping).
#[derive(Debug, Clone)]
pub struct RootElementCode {
    /// The generated code for this root element.
    pub code: String,
    /// Start position in source (for source map).
    pub start: u32,
    /// End position in source (for source map).
    pub end: u32,
}

/// Tracks state for component children (slots vs array format).
#[derive(Debug, Clone)]
pub struct ComponentChildrenState {
    /// Element ID of the component.
    pub element_id: u32,
    /// Position after component opening where children opener should be inserted.
    pub insert_pos: u32,
    /// Position for the separator between props and children (open_tag.end - 1).
    /// Using a separate position allows retroactive _createSlots( wrapping.
    pub separator_pos: u32,
    /// Whether children array/object has been opened.
    pub children_opened: bool,
    /// Whether component uses slots object format (vs array).
    pub uses_slots: bool,
    /// Whether the default slot has been opened (for named slots + default content).
    pub default_slot_opened: bool,
    /// Whether this is a block root (dynamic component with _openBlock wrapper).
    /// Block roots need extra closing paren: ]) vs ])) for children format.
    pub is_block_root: bool,
    /// Whether component has named slots (e.g., `#header`).
    /// Named slots are closed individually by CloseAction::VSlot, so component
    /// close only needs to add stability marker and close the object.
    /// Auto-generated default slots need the array/function closed at component close.
    pub has_named_slots: bool,
    /// Dynamic props that need to be tracked for patch flags (e.g., from v-model).
    pub dynamic_props: Vec<String>,
    /// Number of direct children emitted into the default slot so far.
    /// Used to determine if a comma separator is needed before subsequent children.
    pub default_slot_child_count: u32,
    /// Length of element_id_stack when this component opened.
    /// Used to determine if a child is a DIRECT slot child or nested inside an element.
    pub element_id_stack_len_at_open: usize,
    /// The active_vslot_depth when this component was pushed.
    /// Components opened inside a v-slot template (depth > 0) should handle their own
    /// children normally, unlike the v-slot owner component which uses v-slot mechanism.
    pub vslot_depth_at_open: usize,
    /// Whether this component has conditional (v-if) named slots requiring _createSlots().
    pub has_conditional_named_slots: bool,
    /// Counter for conditional slot keys ("0", "1", etc.).
    pub conditional_slot_key_counter: usize,
    /// Number of entries emitted into the createSlots dynamic array.
    pub create_slots_entry_count: usize,
    /// Tracks the currently-open conditional slot's condition type.
    /// Set when a v-slot template with v-if opens, cleared when VSlot closes.
    pub current_conditional_slot: Option<crate::syntax::types::OxcVConditionType>,
    /// Whether the last conditional slot ternary needs `: undefined` to complete.
    /// Set to true after v-if/v-else-if slot closes, cleared by v-else or new entry.
    pub conditional_slot_needs_undefined: bool,
}

/// Tracks state for slot elements (for proper closing).
#[derive(Debug, Clone)]
pub struct SlotElementState {
    /// Element ID of the slot.
    pub element_id: u32,
    /// Whether the slot has children (fallback content).
    pub has_children: bool,
}

impl TemplateCodegenState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the comment text for v-if fallback comment nodes.
    /// In production mode, returns empty string. In dev mode, returns "v-if".
    pub fn vif_comment_text(&self) -> &'static str {
        if self.is_production {
            ""
        } else {
            "v-if"
        }
    }

    /// Format a patch flag value with optional dev-mode comment.
    /// In production: returns just the number (e.g., "128").
    /// In dev: returns number + comment (e.g., "128 /* KEYED_FRAGMENT */").
    pub fn pflag(&self, value: i32, name: &str) -> String {
        if self.is_production {
            format!("{}", value)
        } else {
            format!("{} /* {} */", value, name)
        }
    }

    /// Format a slot stability marker with optional dev-mode comment.
    /// In production: returns "_: 1". In dev: returns "_: 1 /* STABLE */".
    pub fn slot_flag(&self, value: i32, name: &str) -> String {
        if self.is_production {
            format!("_: {}", value)
        } else {
            format!("_: {} /* {} */", value, name)
        }
    }
}

/// Element currently being built (between OpenTagStart and OpenTagEnd).
#[derive(Debug, Clone)]
pub struct CurrentElement {
    /// Tag name span in source.
    pub tag_name: Span,
    /// Element type (native, component, slot, etc.).
    pub tag_type: SyntaxTagType,
    /// Accumulated props.
    pub props: Vec<PropEntry>,
    /// v-for directive data if present.
    pub v_for: Option<VForInfo>,
    /// v-if/else-if/else directive data if present.
    pub v_if: Option<VIfInfo>,
    /// v-slot directive data if present.
    pub v_slot: Option<VSlotInfo>,
    /// Custom directives (v-focus, v-tooltip, etc.).
    pub custom_directives: Vec<CustomDirectiveEntry>,
    /// v-model directive info if present.
    pub v_model: Option<VModelInfo>,
    /// Whether element has v-once directive.
    pub v_once: bool,
    /// Element ID for tracking.
    pub element_id: u32,
    /// Scope ID from analyzer (for v-for/v-slot).
    pub scope_id: Option<u32>,
    /// Whether element has :key prop.
    pub has_key: bool,
    /// The start position of the opening tag.
    pub start: u32,
}

/// Entry for a custom directive on an element.
#[derive(Debug, Clone)]
pub struct CustomDirectiveEntry {
    /// Directive name without "v-" prefix (e.g., "focus" for v-focus).
    pub name: String,
    /// Optional directive value expression span.
    pub value: Option<Span>,
    /// Optional directive argument (e.g., "top" in v-tooltip:top).
    pub arg: Option<Span>,
    /// Whether the argument is dynamic (e.g., v-tooltip:[pos]).
    pub is_dynamic_arg: bool,
    /// Modifiers (e.g., ["show", "animate"] for v-tooltip.show.animate).
    pub modifiers: Vec<Span>,
}

/// Entry for v-model directive on an element.
#[derive(Debug, Clone)]
pub struct VModelInfo {
    /// The bound value expression span.
    pub value: Option<Span>,
    /// Modifiers: "lazy", "number", "trim".
    pub modifiers: Vec<Span>,
}

/// What to emit when a scope closes.
#[derive(Debug, Clone, Copy)]
pub enum CloseAction {
    /// v-for element: emit `}), PATCH_FLAG))`
    /// `stable` = true when iterable is a constant (setup binding, number, inline literal)
    VFor { keyed: bool, stable: bool },
    /// v-slot: emit slot closing
    VSlot,
}

/// A collected prop entry.
#[derive(Debug, Clone)]
pub struct PropEntry {
    /// Type of prop binding.
    pub kind: PropKind,
    /// Prop name span.
    pub name: Span,
    /// Prop value/expression span.
    pub value: Option<Span>,
    /// Modifiers spans.
    pub modifiers: Vec<Span>,
    /// Is the argument dynamic? (e.g., :[dynamic]="value")
    pub is_dynamic_arg: bool,
}

/// Kind of prop binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    /// Static attribute: class="foo"
    Static,
    /// v-bind: :prop="expr"
    Bind,
    /// v-bind spread: v-bind="obj" (no attribute name)
    BindSpread,
    /// v-on: @event="handler"
    On,
    /// v-model
    Model,
    /// v-show: style display toggle
    Show,
    /// v-html: innerHTML binding
    Html,
    /// v-text: textContent binding
    Text,
}

/// v-for directive information.
#[derive(Debug, Clone)]
pub struct VForInfo {
    /// The iterable expression span (right side of "in").
    pub iterable: Span,
    /// The iterator pattern span (left side of "in").
    pub iterator: Span,
    /// Scope ID from analyzer.
    pub scope_id: u32,
}

/// v-if/else-if/else directive information.
#[derive(Debug, Clone)]
pub struct VIfInfo {
    /// Type of condition.
    pub condition_type: OxcVConditionType,
    /// Expression span (None for v-else).
    pub expression: Option<Span>,
    /// Whether there are sibling conditions following.
    pub has_siblings: bool,
    /// Scope ID from analyzer.
    pub scope_id: u32,
}

/// v-slot directive information.
#[derive(Debug, Clone)]
pub struct VSlotInfo {
    /// Slot name span (None for default slot).
    pub name: Option<Span>,
    /// Slot params span (e.g., "{ item }" in #item="{ item }").
    pub params: Option<Span>,
    /// Scope ID from analyzer.
    pub scope_id: u32,
}

/// Tracks active conditional chain.
#[derive(Debug, Clone)]
pub struct ConditionalChain {
    /// Parent element ID where chain started.
    pub parent_id: u32,
    /// Number of conditions in chain so far.
    pub count: usize,
}

// ============================================================================
// Helper Flags (for required imports)
// ============================================================================

/// Flags indicating which Vue runtime helpers are needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HelperFlags(pub u32);

impl HelperFlags {
    pub const OPEN_BLOCK: u32 = 1 << 0;
    pub const CREATE_ELEMENT_BLOCK: u32 = 1 << 1;
    pub const CREATE_ELEMENT_VNODE: u32 = 1 << 2;
    pub const CREATE_VNODE: u32 = 1 << 3;
    pub const RENDER_LIST: u32 = 1 << 4;
    pub const TO_DISPLAY_STRING: u32 = 1 << 5;
    pub const CREATE_COMMENT_VNODE: u32 = 1 << 6;
    pub const FRAGMENT: u32 = 1 << 7;
    pub const WITH_CTX: u32 = 1 << 8;
    pub const RENDER_SLOT: u32 = 1 << 9;
    pub const NORMALIZE_PROPS: u32 = 1 << 10;
    pub const MERGE_PROPS: u32 = 1 << 11;
    pub const WITH_DIRECTIVES: u32 = 1 << 12;
    pub const RESOLVE_COMPONENT: u32 = 1 << 13;
    pub const WITH_MODIFIERS: u32 = 1 << 14;
    pub const WITH_KEYS: u32 = 1 << 15;
    pub const RESOLVE_DYNAMIC_COMPONENT: u32 = 1 << 16;
    pub const CREATE_BLOCK: u32 = 1 << 17;
    pub const CREATE_TEXT_VNODE: u32 = 1 << 18;
    pub const GUARD_REACTIVE_PROPS: u32 = 1 << 19;
    pub const RESOLVE_DIRECTIVE: u32 = 1 << 20;
    pub const SET_BLOCK_TRACKING: u32 = 1 << 21;
    pub const V_MODEL_TEXT: u32 = 1 << 22;
    pub const V_MODEL_SELECT: u32 = 1 << 23;
    pub const V_MODEL_CHECKBOX: u32 = 1 << 24;
    pub const V_MODEL_RADIO: u32 = 1 << 25;
    pub const V_MODEL_DYNAMIC: u32 = 1 << 26;
    pub const NORMALIZE_CLASS: u32 = 1 << 27;
    pub const NORMALIZE_STYLE: u32 = 1 << 28;
    pub const V_SHOW: u32 = 1 << 29;
    pub const CREATE_SLOTS: u32 = 1 << 30;

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn insert(&mut self, flag: u32) {
        self.0 |= flag;
    }

    #[inline]
    pub fn contains(&self, flag: u32) -> bool {
        (self.0 & flag) != 0
    }

    /// Generate import statement for required helpers.
    pub fn to_import_string(&self) -> String {
        if self.is_empty() {
            return String::new();
        }

        let mut imports = Vec::new();

        if self.contains(Self::OPEN_BLOCK) {
            imports.push("openBlock as _openBlock");
        }
        if self.contains(Self::CREATE_ELEMENT_BLOCK) {
            imports.push("createElementBlock as _createElementBlock");
        }
        if self.contains(Self::CREATE_ELEMENT_VNODE) {
            imports.push("createElementVNode as _createElementVNode");
        }
        if self.contains(Self::CREATE_VNODE) {
            imports.push("createVNode as _createVNode");
        }
        if self.contains(Self::RENDER_LIST) {
            imports.push("renderList as _renderList");
        }
        if self.contains(Self::TO_DISPLAY_STRING) {
            imports.push("toDisplayString as _toDisplayString");
        }
        if self.contains(Self::CREATE_COMMENT_VNODE) {
            imports.push("createCommentVNode as _createCommentVNode");
        }
        if self.contains(Self::CREATE_TEXT_VNODE) {
            imports.push("createTextVNode as _createTextVNode");
        }
        if self.contains(Self::FRAGMENT) {
            imports.push("Fragment as _Fragment");
        }
        if self.contains(Self::WITH_CTX) {
            imports.push("withCtx as _withCtx");
        }
        if self.contains(Self::RENDER_SLOT) {
            imports.push("renderSlot as _renderSlot");
        }
        if self.contains(Self::NORMALIZE_PROPS) {
            imports.push("normalizeProps as _normalizeProps");
        }
        if self.contains(Self::MERGE_PROPS) {
            imports.push("mergeProps as _mergeProps");
        }
        if self.contains(Self::WITH_DIRECTIVES) {
            imports.push("withDirectives as _withDirectives");
        }
        if self.contains(Self::RESOLVE_COMPONENT) {
            imports.push("resolveComponent as _resolveComponent");
        }
        if self.contains(Self::WITH_MODIFIERS) {
            imports.push("withModifiers as _withModifiers");
        }
        if self.contains(Self::WITH_KEYS) {
            imports.push("withKeys as _withKeys");
        }
        if self.contains(Self::RESOLVE_DYNAMIC_COMPONENT) {
            imports.push("resolveDynamicComponent as _resolveDynamicComponent");
        }
        if self.contains(Self::CREATE_BLOCK) {
            imports.push("createBlock as _createBlock");
        }
        if self.contains(Self::GUARD_REACTIVE_PROPS) {
            imports.push("guardReactiveProps as _guardReactiveProps");
        }
        if self.contains(Self::RESOLVE_DIRECTIVE) {
            imports.push("resolveDirective as _resolveDirective");
        }
        if self.contains(Self::SET_BLOCK_TRACKING) {
            imports.push("setBlockTracking as _setBlockTracking");
        }
        if self.contains(Self::V_MODEL_TEXT) {
            imports.push("vModelText as _vModelText");
        }
        if self.contains(Self::V_MODEL_SELECT) {
            imports.push("vModelSelect as _vModelSelect");
        }
        if self.contains(Self::V_MODEL_CHECKBOX) {
            imports.push("vModelCheckbox as _vModelCheckbox");
        }
        if self.contains(Self::V_MODEL_RADIO) {
            imports.push("vModelRadio as _vModelRadio");
        }
        if self.contains(Self::V_MODEL_DYNAMIC) {
            imports.push("vModelDynamic as _vModelDynamic");
        }
        if self.contains(Self::NORMALIZE_CLASS) {
            imports.push("normalizeClass as _normalizeClass");
        }
        if self.contains(Self::NORMALIZE_STYLE) {
            imports.push("normalizeStyle as _normalizeStyle");
        }
        if self.contains(Self::V_SHOW) {
            imports.push("vShow as _vShow");
        }
        if self.contains(Self::CREATE_SLOTS) {
            imports.push("createSlots as _createSlots");
        }

        format!("import {{ {} }} from \"vue\"\n", imports.join(", "))
    }
}

// ============================================================================
// Legacy types (kept for compatibility, may be removed later)
// ============================================================================

/// Result of processing a template element.
#[derive(Debug, Default)]
pub struct ElementProcessResult {
    /// Whether this element should be hoisted (static)
    pub is_static: bool,
    /// Patch flags for this element
    pub patch_flags: u32,
    /// Dynamic prop keys
    pub dynamic_props: Vec<String>,
}

/// Result of processing a directive.
#[derive(Debug, Default)]
pub struct DirectiveProcessResult {
    /// The span to potentially remove or transform
    pub remove_span: Option<Span>,
    /// The code to generate for this directive
    pub generated_code: Option<String>,
}

/// Legacy context - kept for compatibility.
#[derive(Debug, Default)]
pub struct TemplateCodegenContext {
    /// Current indentation level
    pub indent_level: usize,
    /// Whether we're inside a v-for loop
    pub in_v_for: bool,
    /// Whether we're inside a slot
    pub in_slot: bool,
    /// Stack of v-if/else-if/else conditions
    pub condition_stack: Vec<ConditionEntry>,
}

#[derive(Debug, Clone)]
pub struct ConditionEntry {
    pub element_id: u32,
    pub has_else: bool,
}

impl TemplateCodegenContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn indent(&mut self) {
        self.indent_level += 1;
    }

    pub fn dedent(&mut self) {
        if self.indent_level > 0 {
            self.indent_level -= 1;
        }
    }

    pub fn get_indent(&self) -> String {
        "  ".repeat(self.indent_level)
    }
}

/// Patch flag constants matching Vue's runtime.
pub mod patch_flags {
    pub const TEXT: u32 = 1;
    pub const CLASS: u32 = 2;
    pub const STYLE: u32 = 4;
    pub const PROPS: u32 = 8;
    pub const FULL_PROPS: u32 = 16;
    pub const NEED_HYDRATION: u32 = 32;
    pub const STABLE_FRAGMENT: u32 = 64;
    pub const KEYED_FRAGMENT: u32 = 128;
    pub const UNKEYED_FRAGMENT: u32 = 256;
    pub const NEED_PATCH: u32 = 512;
    pub const DYNAMIC_SLOTS: u32 = 1024;
    pub const DEV_ROOT_FRAGMENT: u32 = 2048;
    pub const HOISTED: i32 = -1;
    pub const BAIL: i32 = -2;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build BindingMetadata from a source string and list of (name, type) pairs.
    fn make_metadata(source: &str, bindings: &[(&str, BindingType)]) -> BindingMetadata {
        let mut entries = Vec::new();
        for (name, bt) in bindings {
            if let Some(start) = source.find(name) {
                entries.push((
                    Span {
                        start: start as u32,
                        end: (start + name.len()) as u32,
                    },
                    *bt,
                ));
            }
        }
        BindingMetadata { entries }
    }

    #[test]
    fn test_props_standalone_uses_dollar_props_prefix() {
        let source = "title count";
        let metadata = make_metadata(source, &[("title", BindingType::Props)]);
        let prefix = resolve_binding_prefix(b"title", &metadata, source.as_bytes(), false);
        assert_eq!(
            prefix, "$props.",
            "Props in standalone mode should use $props. prefix"
        );
    }

    #[test]
    fn test_props_inline_uses_dunder_props_prefix() {
        let source = "title count";
        let metadata = make_metadata(source, &[("title", BindingType::Props)]);
        let prefix = resolve_binding_prefix(b"title", &metadata, source.as_bytes(), true);
        assert_eq!(
            prefix, "__props.",
            "Props in inline mode should use __props. prefix"
        );
    }

    #[test]
    fn test_setup_standalone_uses_setup_prefix() {
        let source = "title count";
        let metadata = make_metadata(source, &[("count", BindingType::Setup)]);
        let prefix = resolve_binding_prefix(b"count", &metadata, source.as_bytes(), false);
        assert_eq!(
            prefix, "$setup.",
            "Setup in standalone mode should use $setup. prefix"
        );
    }

    #[test]
    fn test_setup_inline_uses_bare_prefix() {
        let source = "title count";
        let metadata = make_metadata(source, &[("count", BindingType::Setup)]);
        let prefix = resolve_binding_prefix(b"count", &metadata, source.as_bytes(), true);
        assert_eq!(
            prefix, "",
            "Setup in inline mode should be bare (no prefix)"
        );
    }

    #[test]
    fn test_unknown_binding_uses_ctx_prefix() {
        let source = "title count";
        let metadata = BindingMetadata::default();
        let prefix = resolve_binding_prefix(b"unknown", &metadata, source.as_bytes(), false);
        assert_eq!(prefix, "_ctx.", "Unknown binding should use _ctx. prefix");
    }

    #[test]
    fn test_setup_ref_standalone_uses_setup_prefix() {
        let source = "count";
        let metadata = make_metadata(source, &[("count", BindingType::SetupRef)]);
        let prefix = resolve_binding_prefix(b"count", &metadata, source.as_bytes(), false);
        assert_eq!(
            prefix, "$setup.",
            "SetupRef in standalone mode should use $setup. prefix"
        );
        let suffix = resolve_binding_suffix(b"count", &metadata, source.as_bytes(), false);
        assert_eq!(
            suffix, "",
            "SetupRef in standalone mode should have no suffix"
        );
    }

    #[test]
    fn test_setup_ref_inline_uses_bare_prefix_with_value_suffix() {
        let source = "count";
        let metadata = make_metadata(source, &[("count", BindingType::SetupRef)]);
        let prefix = resolve_binding_prefix(b"count", &metadata, source.as_bytes(), true);
        assert_eq!(
            prefix, "",
            "SetupRef in inline mode should be bare (no prefix)"
        );
        let suffix = resolve_binding_suffix(b"count", &metadata, source.as_bytes(), true);
        assert_eq!(
            suffix, ".value",
            "SetupRef in inline mode should have .value suffix"
        );
    }

    #[test]
    fn test_setup_non_ref_inline_has_no_suffix() {
        let source = "myFunc";
        let metadata = make_metadata(source, &[("myFunc", BindingType::Setup)]);
        let suffix = resolve_binding_suffix(b"myFunc", &metadata, source.as_bytes(), true);
        assert_eq!(
            suffix, "",
            "Setup (non-ref) in inline mode should have no suffix"
        );
    }
}
