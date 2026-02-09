//! Main analysis struct that orchestrates template analysis.
//!
//! This struct receives SyntaxEvents from the main syntax pipeline and
//! performs scope analysis and script content extraction, emitting events
//! through an AnalysisPlugin pipeline.

use oxc_span::GetSpan;
use rustc_hash::FxHashMap;

use crate::{
    common::Span,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{
            AnalysedCloseScopes, AnalysedOxcInterpolation, AnalysedOxcProp,
            AnalysedStartScopeVConditional, AnalysedVFor, AnalysedVSlot, AnalysisScriptInfo,
            OxcVConditionType, SyntaxEvent, SyntaxNode, SyntaxTagType, NO_PARENT,
        },
    },
    utils::oxc::{
        vue::{
            parse_script, BindingRefContext, BindingRefInfo, ComponentUsageInfo, IterableType,
            LoopInfo, SlotDefinitionInfo, SlotName, SlotUsageInfo, StaticConditionValue,
            TemplateRefAttrUsage, TemplateUsageCollector,
        },
        BindingExtractionResult,
    },
};

use crate::syntax::types::{
    AnalysisFullScopedCondition, AnalysisProvidedBinding, AnalysisScopeCondition,
    AnalysisScopeEventData, AnalysisScopeStart, AnalysisScopeType,
};

/// Configuration for what tracking features are enabled during analysis.
///
/// Use this to disable tracking features for benchmarking or when the
/// extra information isn't needed.
#[derive(Debug, Clone, Copy)]
pub struct AnalysisConfig {
    /// Track unreachable branches (v-if="false", v-else after v-if="true").
    /// When disabled, loops inside unreachable conditions will still emit warnings.
    pub track_unreachable_branches: bool,

    /// Track render performance warnings (missing :key, nested loops, etc.).
    /// When disabled, no render warnings are collected.
    pub track_render_warnings: bool,

    /// Track static iterable detection (v-for="i in 5").
    /// When disabled, all iterables are treated as dynamic.
    pub track_static_iterables: bool,

    /// Track template metrics (element count, loop depth, etc.).
    /// When disabled, metrics remain at zero.
    pub track_template_metrics: bool,

    /// Track binding references from template to script.
    /// When disabled, binding_refs will be empty.
    pub track_binding_refs: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self::full()
    }
}

impl AnalysisConfig {
    /// Full tracking - all features enabled (default).
    pub const fn full() -> Self {
        Self {
            track_unreachable_branches: true,
            track_render_warnings: true,
            track_static_iterables: true,
            track_template_metrics: true,
            track_binding_refs: true,
        }
    }

    /// Minimal tracking - only core scope analysis, no extras.
    /// Use this for benchmarking baseline performance.
    pub const fn minimal() -> Self {
        Self {
            track_unreachable_branches: false,
            track_render_warnings: false,
            track_static_iterables: false,
            track_template_metrics: false,
            track_binding_refs: false,
        }
    }

    /// Scope-only tracking - scopes + binding refs, no render analysis.
    pub const fn scope_only() -> Self {
        Self {
            track_unreachable_branches: false,
            track_render_warnings: false,
            track_static_iterables: false,
            track_template_metrics: false,
            track_binding_refs: true,
        }
    }
}

/// Main analysis struct that handles scope tracking and script content extraction.
/// Similar to the Syntax struct, it uses a pipeline of AnalysisPlugins to process events.
pub struct Analysis<'a> {
    /// Configuration controlling what features are enabled
    config: AnalysisConfig,

    /// Stack of active scopes for scope hierarchy tracking
    scope_stack: Vec<AnalysisScopeStart<'a>>,

    /// Map of sibling conditions by parent_id for v-if/else chains
    sibling_map: FxHashMap<u32, Vec<AnalysisScopeCondition>>,

    /// Template usage collection for cross-file analysis
    template_usage: TemplateUsageCollector,

    /// Current loop nesting depth (0 = not in a loop)
    loop_depth: u32,

    /// Stack of loop element_ids for parent loop tracking
    loop_stack: Vec<u32>,

    /// Elements that have a v-if/v-else-if/v-else on them
    /// Used to detect v-for + v-if on same element (anti-pattern)
    elements_with_condition: FxHashMap<u32, bool>,

    /// Current conditional chain length (for tracking max chain)
    current_chain_length: u32,

    /// Depth of unreachable code (0 = reachable, >0 = inside v-if="false" or similar)
    /// Incremented when entering unreachable branch, decremented when leaving
    unreachable_depth: u32,

    /// Static condition values by parent_id (for tracking siblings after v-if="true")
    /// Maps parent_id -> StaticConditionValue of the first v-if in chain
    static_condition_by_parent: FxHashMap<u32, StaticConditionValue>,

    /// Elements that caused unreachable_depth to increment.
    /// When closing these elements, we decrement unreachable_depth.
    unreachable_entry_elements: rustc_hash::FxHashSet<u32>,
}

#[allow(clippy::new_without_default)]
impl<'a> Analysis<'a> {
    /// Create a new Analysis with default (full) tracking configuration.
    pub fn new() -> Self {
        Self::with_config(AnalysisConfig::default())
    }

    /// Create a new Analysis with a specific configuration.
    pub fn with_config(config: AnalysisConfig) -> Self {
        Self {
            config,
            scope_stack: Vec::with_capacity(5),
            sibling_map: FxHashMap::default(),
            template_usage: TemplateUsageCollector::new(),
            loop_depth: 0,
            loop_stack: Vec::with_capacity(3),
            elements_with_condition: FxHashMap::default(),
            current_chain_length: 0,
            unreachable_depth: 0,
            static_condition_by_parent: FxHashMap::default(),
            unreachable_entry_elements: rustc_hash::FxHashSet::default(),
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &AnalysisConfig {
        &self.config
    }

    /// Check if we're currently inside an unreachable branch
    fn is_in_unreachable_branch(&self) -> bool {
        self.unreachable_depth > 0
    }

    /// Get a reference to the collected template usage
    pub fn template_usage(&self) -> &TemplateUsageCollector {
        &self.template_usage
    }

    /// Take ownership of the collected template usage, leaving an empty collector
    pub fn take_template_usage(&mut self) -> TemplateUsageCollector {
        self.template_usage.take()
    }

    /// Get the current scope ID (for binding reference tracking)
    fn current_scope_id(&self) -> u32 {
        self.scope_stack.last().map(|s| s.id).unwrap_or(NO_PARENT)
    }

    // ==================== Scope Tracking Methods ====================

    /// Process sibling conditions for a scope.
    fn process_sibling_conditions(
        &mut self,
        condition: &mut Option<AnalysisFullScopedCondition>,
        parent_id: u32,
    ) {
        // Look up sibling conditions from the map
        if let Some(siblings) = self.sibling_map.get(&parent_id) {
            if condition.is_none() {
                *condition = Some(AnalysisFullScopedCondition {
                    value: None,
                    siblings: siblings.to_vec(),
                });
            } else {
                condition
                    .as_mut()
                    .unwrap()
                    .siblings
                    .extend_from_slice(siblings);
            }
        }

        // If this scope has NO condition value, it breaks the sibling chain
        let has_condition_value = condition
            .as_ref()
            .map(|c| c.value.is_some())
            .unwrap_or(false);

        if !has_condition_value {
            self.sibling_map.remove(&parent_id);
        }

        // Add condition to the sibling map for future siblings
        if let Some(condition) = condition {
            if let Some(value) = &condition.value {
                self.sibling_map.entry(parent_id).or_default().push(*value);
            }
        }
    }

    /// Inherit bindings, provided_bindings, and conditions from parent scope.
    fn inherit_from_parent(
        &self,
    ) -> (
        u32,
        BindingExtractionResult<'a>,
        Vec<AnalysisProvidedBinding>,
        Vec<AnalysisFullScopedCondition>,
    ) {
        if let Some(parent_scope) = self.scope_stack.last() {
            let mut parent_bindings = BindingExtractionResult::default();
            parent_bindings.extend(&parent_scope.parent_bindings);
            parent_bindings.extend(&parent_scope.bindings);

            let mut provided_bindings = Vec::new();
            provided_bindings.extend_from_slice(&parent_scope.provided_bindings);
            // Only propagate bindings from scopes that introduce new variables.
            // Loop (v-for) and Slot (v-slot) scopes create new local bindings.
            // Conditional (v-if) and other scopes only USE identifiers in their
            // expressions — they don't introduce new variables for children.
            let introduces_bindings = matches!(
                parent_scope.r#type,
                AnalysisScopeType::Loop | AnalysisScopeType::Slot
            );
            if introduces_bindings && !parent_scope.bindings.bindings.is_empty() {
                provided_bindings.push(AnalysisProvidedBinding {
                    scope_id: parent_scope.id,
                    element_id: parent_scope.element_id,
                    // Use absolute spans: b.pos is the absolute start position,
                    // and b.name.len() gives the byte length of the identifier.
                    // Note: b.span is relative to the parsed substring, NOT absolute!
                    spans: parent_scope
                        .bindings
                        .bindings
                        .iter()
                        .map(|b| Span::new(b.pos, b.pos + b.name.len() as u32))
                        .collect(),
                });
            }

            let mut parent_conditions = Vec::new();
            if let Some(ref parent_condition) = parent_scope.condition {
                parent_conditions.push(parent_condition.clone());
            }
            parent_conditions.extend_from_slice(&parent_scope.parent_conditions);

            (
                parent_scope.id,
                parent_bindings,
                provided_bindings,
                parent_conditions,
            )
        } else {
            (
                NO_PARENT,
                BindingExtractionResult::default(),
                Vec::new(),
                Vec::new(),
            )
        }
    }

    /// Starts a deep scope (v-for, v-slot)
    fn start_scope(&mut self, mut scope: AnalysisScopeStart<'a>) -> AnalysisScopeStart<'a> {
        let (parent_scope_id, parent_bindings, provided_bindings, parent_conditions) =
            self.inherit_from_parent();

        scope.parent_scope_id = parent_scope_id;
        scope.parent_bindings.extend(&parent_bindings);
        scope
            .provided_bindings
            .extend_from_slice(&provided_bindings);
        scope
            .parent_conditions
            .extend_from_slice(&parent_conditions);

        self.process_sibling_conditions(&mut scope.condition, scope.parent_id);

        self.scope_stack.push(scope.clone());
        scope
    }

    fn close_scope(&mut self, parent_id: u32) -> Vec<u32> {
        self.sibling_map.remove(&parent_id);

        // Clean up condition tracking for this element
        self.elements_with_condition.remove(&parent_id);

        // If this element was an unreachable entry point, decrement the depth
        if self.unreachable_entry_elements.remove(&parent_id) {
            self.unreachable_depth = self.unreachable_depth.saturating_sub(1);
        }

        // Clean up static condition tracking when leaving the parent scope
        // (siblings are now complete)
        self.static_condition_by_parent.remove(&parent_id);

        // If we're closing a loop, update loop tracking
        if self.loop_stack.last() == Some(&parent_id) {
            self.loop_stack.pop();
            self.loop_depth = self.loop_depth.saturating_sub(1);
        }

        let mut closed_scopes = Vec::new();
        while !self.scope_stack.is_empty() {
            let last = self.scope_stack.last();

            match last {
                Some(scope) if scope.element_id == parent_id => {
                    if let Some(scope) = self.scope_stack.pop() {
                        closed_scopes.push(scope.id);
                    }
                }
                _ => {
                    return closed_scopes;
                }
            }
        }
        closed_scopes
    }

    fn event(&mut self, mut scope: AnalysisScopeEventData<'a>) -> AnalysisScopeEventData<'a> {
        let (parent_scope_id, parent_bindings, provided_bindings, parent_conditions) =
            self.inherit_from_parent();

        scope.parent_scope_id = parent_scope_id;
        scope.parent_bindings = Some(parent_bindings);
        scope.provided_bindings = Some(provided_bindings);
        scope
            .parent_conditions
            .extend_from_slice(&parent_conditions);

        self.process_sibling_conditions(&mut scope.condition, scope.parent_id);

        scope
    }

    // ==================== Template Usage Collection ====================

    /// Collect ref attribute usage from a prop event
    fn collect_ref_attr_usage(
        &mut self,
        prop: &crate::syntax::types::OxcProp<'a>,
        ctx: &SyntaxPluginContext<'a>,
    ) {
        let event = &prop.event;

        if !event.is_directive {
            // Check for static ref="name" attribute
            let name_bytes = &ctx.bytes[event.start as usize..event.name_end as usize];
            if name_bytes == b"ref" {
                if let Some(value) = &event.value {
                    self.template_usage.record_ref_attr(TemplateRefAttrUsage {
                        name_span: Span::new(value.start, value.end),
                        element_id: event.element_id,
                        is_dynamic: false,
                    });
                }
            }
        } else {
            // Check for dynamic :ref="expr" or v-bind:ref="expr"
            if let Some(arg) = &event.arg {
                let arg_bytes = &ctx.bytes[arg.start as usize..arg.end as usize];
                if arg_bytes == b"ref" {
                    if let Some(value) = &event.value {
                        self.template_usage.record_ref_attr(TemplateRefAttrUsage {
                            name_span: Span::new(value.start, value.end),
                            element_id: event.element_id,
                            is_dynamic: true,
                        });
                    }
                }
            }
        }
    }

    /// Collect slot usage from a v-slot event
    fn collect_slot_usage(
        &mut self,
        slot: &crate::syntax::types::OxcVSlotProp<'a>,
        _ctx: &SyntaxPluginContext<'a>,
    ) {
        let event = &slot.event;

        // Determine slot name
        let name = if let Some(arg) = &event.arg {
            if arg.is_dynamic {
                SlotName::Dynamic {
                    span: Span::new(arg.start, arg.end),
                }
            } else {
                SlotName::Named {
                    span: Span::new(arg.start, arg.end),
                }
            }
        } else {
            SlotName::Default
        };

        // Collect scope binding spans
        let scope_binding_spans: Vec<Span> = slot
            .locals()
            .iter()
            .map(|local| {
                let value_offset = event.value.as_ref().map(|v| v.start).unwrap_or(0);
                Span::new(value_offset + local.start, value_offset + local.end)
            })
            .collect();

        self.template_usage.record_slot_usage(SlotUsageInfo {
            span: Span::new(event.start, event.end),
            name,
            element_id: slot.element_id,
            scope_binding_spans,
        });
    }

    // ==================== Binding Reference Collection ====================

    /// Collect binding references from a BindingExtractionResult.
    /// This records which script bindings are referenced in the template.
    fn collect_binding_refs(
        &mut self,
        bindings: &BindingExtractionResult<'_>,
        element_id: u32,
        context: BindingRefContext,
    ) {
        // Skip if binding ref tracking is disabled
        if !self.config.track_binding_refs {
            return;
        }

        let scope_id = self.current_scope_id();

        for binding in &bindings.bindings {
            // Skip ignored bindings (like globals or locally shadowed vars)
            if binding.ignore {
                continue;
            }

            self.template_usage.record_binding_ref(BindingRefInfo {
                name_span: binding.span,
                element_id,
                scope_id,
                context,
            });
        }
    }

    /// Determine the binding context for a directive based on its name.
    fn get_directive_context(name_bytes: &[u8]) -> BindingRefContext {
        // Check for event handlers: v-on:xxx, @xxx
        if name_bytes.starts_with(b"v-on") || name_bytes.starts_with(b"@") {
            return BindingRefContext::EventHandler;
        }
        BindingRefContext::DirectiveValue
    }

    /// Handle a syntax event and emit analysis events through the pipeline
    pub fn handle(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match event {
            SyntaxEvent::OxcScriptContent(e) => {
                let mode = if e.setup.is_some() {
                    crate::utils::oxc::vue::ScriptMode::Setup
                } else {
                    crate::utils::oxc::vue::ScriptMode::Options
                };

                let result = parse_script(&e.program, mode, e.content_start, ctx.input);
                let ev = AnalysisScriptInfo {
                    event: e,
                    parsed: result,
                };
                SyntaxResult::replace(SyntaxEvent::AnalysedScript(ev))
            }
            SyntaxEvent::OxcProp(e) => {
                // Collect ref attribute usage
                self.collect_ref_attr_usage(&e, ctx);

                // Determine context for binding refs based on directive name
                let directive_context = {
                    let event = &e.event;
                    let name_bytes = &ctx.bytes[event.start as usize..event.name_end as usize];

                    // Check for :key or v-bind:key on loop elements
                    if name_bytes == b":key" || name_bytes == b"v-bind:key" {
                        // Find the loop for this element and mark it as having a key
                        if let Some(loop_info) = self
                            .template_usage
                            .loops
                            .iter_mut()
                            .rev()
                            .find(|l| l.element_id == e.element_id)
                        {
                            loop_info.has_key = true;
                        }
                    }

                    Self::get_directive_context(name_bytes)
                };

                // Increment directive count for metrics
                if self.config.track_template_metrics {
                    self.template_usage.increment_directive_count();
                }

                let mut arg_event = None;
                let mut exp_event = None;
                if let Some(arg) = e.arg.as_ref() {
                    if let Some(bindings) = &arg.bindings {
                        // Collect binding references for dynamic directive arguments
                        self.collect_binding_refs(
                            bindings,
                            e.element_id,
                            BindingRefContext::DirectiveArg,
                        );

                        arg_event = Some(self.event(AnalysisScopeEventData {
                            r#type: AnalysisScopeType::DirectiveArg,
                            start: arg.start,
                            end: arg.end,

                            parent_id: e.parent_id,
                            element_id: e.element_id,
                            bindings: bindings.clone(),
                            condition: None,

                            parent_scope_id: NO_PARENT,
                            parent_bindings: None,
                            provided_bindings: None,
                            parent_conditions: Vec::new(),
                        }));
                    }
                }

                if let Some(exp) = e.exp.as_ref() {
                    if let Some(bindings) = &exp.bindings {
                        // Collect binding references (event handler or directive value)
                        self.collect_binding_refs(bindings, e.element_id, directive_context);

                        exp_event = Some(self.event(AnalysisScopeEventData {
                            r#type: AnalysisScopeType::DirectiveExp,
                            start: exp.start,
                            end: exp.end,

                            parent_id: e.parent_id,
                            element_id: e.element_id,
                            bindings: bindings.clone(),
                            condition: None,

                            parent_scope_id: NO_PARENT,
                            parent_bindings: None,
                            provided_bindings: None,
                            parent_conditions: Vec::new(),
                        }));
                    }
                }
                let ev = AnalysedOxcProp {
                    event: e,
                    arg: arg_event,
                    exp: exp_event,
                };
                SyntaxResult::replace(SyntaxEvent::AnalysedProp(ev))
            }

            SyntaxEvent::OxcInterpolation(e) => {
                // Increment interpolation count for metrics
                if self.config.track_template_metrics {
                    self.template_usage.increment_interpolation_count();
                }

                let mut interpolation = None;
                if let Some(bindings) = &e.bindings {
                    // Collect binding references for unused variable detection
                    self.collect_binding_refs(
                        bindings,
                        e.parent_id, // parent_id is the containing element
                        BindingRefContext::Interpolation,
                    );

                    interpolation = Some(self.event(AnalysisScopeEventData {
                        r#type: AnalysisScopeType::Interpolation,
                        start: e.start,
                        end: e.end,

                        parent_id: e.parent_id,
                        element_id: NO_PARENT,
                        bindings: bindings.clone(),
                        condition: None,

                        parent_scope_id: NO_PARENT,
                        parent_bindings: None,
                        provided_bindings: None,
                        parent_conditions: Vec::new(),
                    }));
                }

                let ev = AnalysedOxcInterpolation {
                    event: e,
                    interpolation,
                };
                SyntaxResult::replace(SyntaxEvent::AnalysedInterpolation(ev))
            }

            SyntaxEvent::OxcVConditional(e) => {
                // Mark this element as having a condition (for v-for + v-if detection)
                if self.config.track_render_warnings {
                    self.elements_with_condition.insert(e.element_id, true);
                }

                // Track conditional chain length for metrics
                if self.config.track_template_metrics {
                    self.template_usage
                        .record_conditional(self.current_chain_length + 1);
                }

                // Collect binding references for v-if/v-else-if conditions
                if let Some(bindings) = &e.bindings {
                    self.collect_binding_refs(
                        bindings,
                        e.element_id,
                        BindingRefContext::DirectiveValue,
                    );
                }

                // Track unreachable branches (only if enabled)
                if self.config.track_unreachable_branches {
                    // Detect static condition value for unreachable branch tracking
                    let static_value = if let Some(exp) = &e.expression {
                        let expr_bytes =
                            &ctx.bytes[exp.span().start as usize..exp.span().end as usize];
                        StaticConditionValue::from_expression_bytes(expr_bytes)
                    } else {
                        // v-else has no expression
                        StaticConditionValue::Dynamic
                    };

                    // Track unreachable branches based on condition type and static value
                    match e.condition_type {
                        OxcVConditionType::If => {
                            // v-if="false" makes this branch unreachable
                            if static_value.is_unreachable() {
                                self.unreachable_depth += 1;
                                self.unreachable_entry_elements.insert(e.element_id);
                            }
                            // v-if="true" makes siblings unreachable
                            if static_value.siblings_unreachable() {
                                self.static_condition_by_parent
                                    .insert(e.parent_id, static_value);
                            }
                        }
                        OxcVConditionType::ElseIf => {
                            // Check if preceding v-if was always true (making this unreachable)
                            let prev_was_always_true = self
                                .static_condition_by_parent
                                .get(&e.parent_id)
                                .map(|v| v.siblings_unreachable())
                                .unwrap_or(false);

                            if prev_was_always_true {
                                // Sibling of v-if="true" - unreachable
                                self.unreachable_depth += 1;
                                self.unreachable_entry_elements.insert(e.element_id);
                            } else if static_value.is_unreachable() {
                                // This v-else-if is itself always false
                                self.unreachable_depth += 1;
                                self.unreachable_entry_elements.insert(e.element_id);
                            } else if static_value.siblings_unreachable() {
                                // v-else-if="true" makes remaining siblings unreachable
                                self.static_condition_by_parent
                                    .insert(e.parent_id, static_value);
                            }
                        }
                        OxcVConditionType::Else => {
                            // Check if preceding v-if/v-else-if was always true
                            let prev_was_always_true = self
                                .static_condition_by_parent
                                .get(&e.parent_id)
                                .map(|v| v.siblings_unreachable())
                                .unwrap_or(false);

                            if prev_was_always_true {
                                self.unreachable_depth += 1;
                                self.unreachable_entry_elements.insert(e.element_id);
                            }
                        }
                    }
                }

                // Always emit scope for conditionals (even v-else with no bindings)
                let bindings = e
                    .bindings
                    .clone()
                    .unwrap_or_else(BindingExtractionResult::default);

                let condition = if let Some(exp) = &e.expression {
                    Some(AnalysisFullScopedCondition {
                        value: Some(AnalysisScopeCondition {
                            condition_type: e.condition_type,
                            start: e.start,
                            end: e.end,
                            expression_start: exp.span().start,
                            expression_end: exp.span().end,
                        }),
                        siblings: Vec::new(),
                    })
                } else {
                    None
                };

                let scope = self.start_scope(AnalysisScopeStart {
                    id: e.get_id(),
                    r#type: AnalysisScopeType::Conditional,

                    parent_id: e.parent_id,
                    element_id: e.element_id,
                    bindings,
                    condition,

                    parent_scope_id: NO_PARENT,
                    parent_bindings: BindingExtractionResult::default(),
                    provided_bindings: Vec::new(),
                    parent_conditions: Vec::new(),
                });

                let ev = AnalysedStartScopeVConditional { event: e, scope };

                SyntaxResult::replace(SyntaxEvent::AnalysedCondition(ev))
            }

            SyntaxEvent::CloseTag(e) => {
                let scopes = self.close_scope(e.element_id);
                let ev = AnalysedCloseScopes {
                    event: e,
                    closed_scope_ids: scopes,
                };
                SyntaxResult::replace(SyntaxEvent::AnalysedCloseScopes(ev))
            }

            SyntaxEvent::OxcVFor(e) => {
                // Track loop depth and parent loop
                self.loop_depth += 1;
                let parent_loop_id = self.loop_stack.last().copied();
                self.loop_stack.push(e.element_id);

                // Check if this element also has a v-if (anti-pattern)
                let has_condition_on_same =
                    self.elements_with_condition.contains_key(&e.element_id);

                let value_offset = e.event.value.as_ref().map(|v| v.start).unwrap_or(0);

                let bindings = BindingExtractionResult {
                    bindings: e
                        .locals()
                        .iter()
                        .map(|span| {
                            let abs_start = value_offset + span.start;
                            let abs_end = value_offset + span.end;
                            crate::utils::oxc::Binding {
                                name: ctx.slice(abs_start, abs_end),
                                span: Span::new(abs_start, abs_end),
                                pos: abs_start,
                                ignore: false,
                            }
                        })
                        .collect(),
                    ..Default::default()
                };

                let scope = self.start_scope(AnalysisScopeStart {
                    id: e.start,
                    r#type: AnalysisScopeType::Loop,
                    parent_id: e.parent_id,
                    element_id: e.element_id,
                    parent_scope_id: NO_PARENT,
                    bindings,
                    parent_bindings: BindingExtractionResult::default(),
                    provided_bindings: Vec::new(),
                    condition: None,
                    parent_conditions: Vec::new(),
                });

                // Only record loop info if render warnings are enabled
                if self.config.track_render_warnings {
                    // Compute iterable span and type from the right expression
                    let (iterable_span, iterable_type) = if let Some(right) = e.right() {
                        let right_offset = e.parsed.result.right_offset;
                        let abs_start = value_offset + right_offset;
                        let abs_end = value_offset + right_offset + right.span().size();
                        let span = Span::new(abs_start, abs_end);

                        // Get the iterable expression bytes to detect static numbers (if enabled)
                        let iterable_type = if self.config.track_static_iterables {
                            let iterable_bytes = &ctx.bytes[abs_start as usize..abs_end as usize];
                            IterableType::from_expression_bytes(iterable_bytes)
                        } else {
                            IterableType::Dynamic
                        };

                        (span, iterable_type)
                    } else if let Some(first_ref) = e.references().first() {
                        // Fallback: use references if right expression not available
                        let abs_start = value_offset + first_ref.start;
                        let abs_end = value_offset + first_ref.end;
                        (Span::new(abs_start, abs_end), IterableType::Dynamic)
                    } else {
                        (Span::new(e.event.start, e.event.end), IterableType::Dynamic)
                    };

                    // Record loop info for render performance analysis
                    // Note: has_key will be updated later when we see the :key attribute
                    self.template_usage.record_loop(LoopInfo {
                        span: Span::new(e.event.start, e.event.end),
                        element_id: e.element_id,
                        depth: self.loop_depth,
                        parent_loop_id,
                        has_key: false, // Will be updated when we see :key attribute
                        has_condition_on_same,
                        iterable_span,
                        iterable_type,
                        in_unreachable_branch: self.config.track_unreachable_branches
                            && self.is_in_unreachable_branch(),
                    });
                }

                let references = if !e.references().is_empty() {
                    let ref_bindings = BindingExtractionResult {
                        bindings: e
                            .references()
                            .iter()
                            .map(|span| {
                                let abs_start = value_offset + span.start;
                                let abs_end = value_offset + span.end;
                                crate::utils::oxc::Binding {
                                    name: ctx.slice(abs_start, abs_end),
                                    span: Span::new(abs_start, abs_end),
                                    pos: abs_start,
                                    ignore: false,
                                }
                            })
                            .collect(),
                        ..Default::default()
                    };

                    // Collect binding references for v-for iterable expression
                    self.collect_binding_refs(
                        &ref_bindings,
                        e.element_id,
                        BindingRefContext::DirectiveValue,
                    );

                    Some(self.event(AnalysisScopeEventData {
                        r#type: AnalysisScopeType::DirectiveExp,
                        start: e.event.start,
                        end: e.event.end,
                        parent_id: e.parent_id,
                        element_id: e.element_id,
                        bindings: ref_bindings,
                        condition: None,
                        parent_scope_id: NO_PARENT,
                        parent_bindings: None,
                        provided_bindings: None,
                        parent_conditions: Vec::new(),
                    }))
                } else {
                    None
                };

                let ev = AnalysedVFor {
                    event: e,
                    scope,
                    references,
                };
                SyntaxResult::replace(SyntaxEvent::AnalysedVFor(ev))
            }

            SyntaxEvent::OxcVSlot(e) => {
                // Collect slot usage
                self.collect_slot_usage(&e, ctx);

                let value_offset = e.event.value.as_ref().map(|v| v.start).unwrap_or(0);

                let bindings = BindingExtractionResult {
                    bindings: e
                        .locals()
                        .iter()
                        .map(|span| {
                            let abs_start = value_offset + span.start;
                            let abs_end = value_offset + span.end;
                            crate::utils::oxc::Binding {
                                name: ctx.slice(abs_start, abs_end),
                                span: Span::new(abs_start, abs_end),
                                pos: abs_start,
                                ignore: false,
                            }
                        })
                        .collect(),
                    ..Default::default()
                };

                let scope = self.start_scope(AnalysisScopeStart {
                    id: e.start,
                    r#type: AnalysisScopeType::Slot,
                    parent_id: e.parent_id,
                    element_id: e.element_id,
                    parent_scope_id: NO_PARENT,
                    bindings,
                    parent_bindings: BindingExtractionResult::default(),
                    provided_bindings: Vec::new(),
                    condition: None,
                    parent_conditions: Vec::new(),
                });

                let references = if !e.references().is_empty() {
                    let ref_bindings = BindingExtractionResult {
                        bindings: e
                            .references()
                            .iter()
                            .map(|span| {
                                let abs_start = value_offset + span.start;
                                let abs_end = value_offset + span.end;
                                crate::utils::oxc::Binding {
                                    name: ctx.slice(abs_start, abs_end),
                                    span: Span::new(abs_start, abs_end),
                                    pos: abs_start,
                                    ignore: false,
                                }
                            })
                            .collect(),
                        ..Default::default()
                    };

                    // Collect binding references for v-slot expressions
                    self.collect_binding_refs(
                        &ref_bindings,
                        e.element_id,
                        BindingRefContext::DirectiveValue,
                    );

                    Some(self.event(AnalysisScopeEventData {
                        r#type: AnalysisScopeType::DirectiveExp,
                        start: e.event.start,
                        end: e.event.end,
                        parent_id: e.parent_id,
                        element_id: e.element_id,
                        bindings: ref_bindings,
                        condition: None,
                        parent_scope_id: NO_PARENT,
                        parent_bindings: None,
                        provided_bindings: None,
                        parent_conditions: Vec::new(),
                    }))
                } else {
                    None
                };

                let ev = AnalysedVSlot {
                    event: e,
                    scope,
                    references,
                };
                SyntaxResult::replace(SyntaxEvent::AnalysedVSlot(ev))
            }

            SyntaxEvent::OpenTagStart(e) => {
                // Increment element count for metrics
                if self.config.track_template_metrics {
                    self.template_usage.increment_element_count();
                }

                // Collect component usage
                if e.tag_type == SyntaxTagType::Component {
                    self.template_usage
                        .record_component_usage(ComponentUsageInfo {
                            span: Span::new(e.start, e.name_end),
                            name_span: Span::new(e.start + 1, e.name_end), // Skip '<'
                            element_id: e.element_id,
                            is_dynamic: false, // TODO: detect <component :is="...">
                        });
                }

                // Collect slot definitions (<slot> elements)
                if e.tag_type == SyntaxTagType::Slot {
                    self.template_usage
                        .record_slot_definition(SlotDefinitionInfo {
                            span: Span::new(e.start, e.name_end),
                            name_span: None, // Will be populated when we see name attribute
                            element_id: e.element_id,
                        });
                }

                SyntaxResult::keep(SyntaxEvent::OpenTagStart(e))
            }

            SyntaxEvent::OpenTagEnd(e) => {
                if e.self_closing || e.is_void_element {
                    // Close scopes for self-closing elements (using element_id, not position)
                    // The scope_ids are closed here; codegen handles scope close actions
                    // when processing self-closing elements in process_open_tag_end
                    self.close_scope(e.element_id);
                }
                SyntaxResult::keep(SyntaxEvent::OpenTagEnd(e))
            }

            other => SyntaxResult::keep(other),
        }
    }
}

impl<'a> SyntaxPlugin<'a> for Analysis<'a> {
    fn name(&self) -> &str {
        "analysis"
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        self.handle(event, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;
    use crate::syntax::types::OxcVConditionType;
    use crate::utils::oxc::Binding;

    // ==================== Helper Functions ====================

    fn make_binding(name: &'static str, start: u32, end: u32) -> Binding<'static> {
        Binding {
            name,
            span: Span::new(start, end),
            pos: start,
            ignore: false,
        }
    }

    fn make_scope<'a>(
        id: u32,
        element_id: u32,
        parent_id: u32,
        scope_type: AnalysisScopeType,
        bindings: Vec<Binding<'a>>,
    ) -> AnalysisScopeStart<'a> {
        AnalysisScopeStart {
            id,
            r#type: scope_type,
            parent_id,
            element_id,
            parent_scope_id: NO_PARENT,
            bindings: BindingExtractionResult {
                bindings,
                ..Default::default()
            },
            parent_bindings: BindingExtractionResult::default(),
            provided_bindings: Vec::new(),
            condition: None,
            parent_conditions: Vec::new(),
        }
    }

    fn make_condition(
        condition_type: OxcVConditionType,
        start: u32,
        end: u32,
    ) -> AnalysisFullScopedCondition {
        AnalysisFullScopedCondition {
            value: Some(AnalysisScopeCondition {
                condition_type,
                start,
                end,
                expression_start: start + 5,
                expression_end: end - 1,
            }),
            siblings: Vec::new(),
        }
    }

    // ==================== inherit_from_parent Tests ====================

    #[test]
    fn test_inherit_from_parent_empty_stack() {
        let analysis = Analysis::new();
        let (parent_id, bindings, provided, conditions) = analysis.inherit_from_parent();

        assert_eq!(parent_id, NO_PARENT);
        assert!(bindings.bindings.is_empty());
        assert!(provided.is_empty());
        assert!(conditions.is_empty());
    }

    #[test]
    fn test_inherit_from_parent_single_scope() {
        let mut analysis = Analysis::new();

        // Manually push a scope with bindings
        let scope = AnalysisScopeStart {
            id: 10,
            r#type: AnalysisScopeType::Loop,
            parent_id: NO_PARENT,
            element_id: 5,
            parent_scope_id: NO_PARENT,
            bindings: BindingExtractionResult {
                bindings: vec![make_binding("item", 100, 104)],
                ..Default::default()
            },
            parent_bindings: BindingExtractionResult::default(),
            provided_bindings: Vec::new(),
            condition: None,
            parent_conditions: Vec::new(),
        };
        analysis.scope_stack.push(scope);

        let (parent_id, bindings, provided, conditions) = analysis.inherit_from_parent();

        assert_eq!(parent_id, 10);
        assert_eq!(bindings.bindings.len(), 1);
        assert_eq!(bindings.bindings[0].name, "item");
        assert_eq!(provided.len(), 1);
        assert_eq!(provided[0].scope_id, 10);
        assert!(conditions.is_empty());
    }

    #[test]
    fn test_inherit_from_parent_nested_scopes() {
        let mut analysis = Analysis::new();

        // Push outer scope
        let outer = AnalysisScopeStart {
            id: 10,
            r#type: AnalysisScopeType::Loop,
            parent_id: NO_PARENT,
            element_id: 5,
            parent_scope_id: NO_PARENT,
            bindings: BindingExtractionResult {
                bindings: vec![make_binding("outer", 100, 105)],
                ..Default::default()
            },
            parent_bindings: BindingExtractionResult::default(),
            provided_bindings: Vec::new(),
            condition: None,
            parent_conditions: Vec::new(),
        };
        analysis.scope_stack.push(outer);

        // Push inner scope (simulating what start_scope does)
        let inner = AnalysisScopeStart {
            id: 20,
            r#type: AnalysisScopeType::Loop,
            parent_id: 5,
            element_id: 15,
            parent_scope_id: 10,
            bindings: BindingExtractionResult {
                bindings: vec![make_binding("inner", 200, 205)],
                ..Default::default()
            },
            parent_bindings: BindingExtractionResult {
                bindings: vec![make_binding("outer", 100, 105)],
                ..Default::default()
            },
            provided_bindings: vec![AnalysisProvidedBinding {
                scope_id: 10,
                element_id: 5,
                spans: vec![Span::new(100, 105)],
            }],
            condition: None,
            parent_conditions: Vec::new(),
        };
        analysis.scope_stack.push(inner);

        let (parent_id, bindings, provided, _) = analysis.inherit_from_parent();

        assert_eq!(parent_id, 20);
        // Should have both outer and inner bindings
        assert_eq!(bindings.bindings.len(), 2);
        // Should have both provided bindings
        assert_eq!(provided.len(), 2);
    }

    // ==================== process_sibling_conditions Tests ====================

    #[test]
    fn test_sibling_conditions_if_starts_chain() {
        let mut analysis = Analysis::new();

        let mut condition = Some(make_condition(OxcVConditionType::If, 0, 20));
        analysis.process_sibling_conditions(&mut condition, 100);

        // v-if should be added to sibling map
        assert!(analysis.sibling_map.contains_key(&100));
        assert_eq!(analysis.sibling_map[&100].len(), 1);
        assert!(matches!(
            analysis.sibling_map[&100][0].condition_type,
            OxcVConditionType::If
        ));
    }

    #[test]
    fn test_sibling_conditions_else_if_sees_if() {
        let mut analysis = Analysis::new();

        // First, add v-if
        let mut if_cond = Some(make_condition(OxcVConditionType::If, 0, 20));
        analysis.process_sibling_conditions(&mut if_cond, 100);

        // Then process v-else-if
        let mut else_if_cond = Some(make_condition(OxcVConditionType::ElseIf, 30, 50));
        analysis.process_sibling_conditions(&mut else_if_cond, 100);

        // v-else-if should now have v-if as sibling
        let cond = else_if_cond.unwrap();
        assert_eq!(cond.siblings.len(), 1);
        assert!(matches!(
            cond.siblings[0].condition_type,
            OxcVConditionType::If
        ));
    }

    #[test]
    fn test_sibling_conditions_else_sees_all() {
        let mut analysis = Analysis::new();

        // Add v-if
        let mut if_cond = Some(make_condition(OxcVConditionType::If, 0, 20));
        analysis.process_sibling_conditions(&mut if_cond, 100);

        // Add v-else-if
        let mut else_if_cond = Some(make_condition(OxcVConditionType::ElseIf, 30, 50));
        analysis.process_sibling_conditions(&mut else_if_cond, 100);

        // Process v-else (no condition value, just siblings)
        let mut else_cond: Option<AnalysisFullScopedCondition> = None;
        analysis.process_sibling_conditions(&mut else_cond, 100);

        // v-else should see both v-if and v-else-if as siblings
        let cond = else_cond.unwrap();
        assert_eq!(cond.siblings.len(), 2);
        assert!(cond.value.is_none()); // v-else has no condition expression
    }

    #[test]
    fn test_sibling_conditions_break_chain() {
        let mut analysis = Analysis::new();

        // Add v-if
        let mut if_cond = Some(make_condition(OxcVConditionType::If, 0, 20));
        analysis.process_sibling_conditions(&mut if_cond, 100);

        // Non-conditional element breaks the chain
        let mut no_cond: Option<AnalysisFullScopedCondition> = None;
        analysis.process_sibling_conditions(&mut no_cond, 100);

        // Sibling map should be cleared for this parent
        assert!(!analysis.sibling_map.contains_key(&100));
    }

    // ==================== start_scope Tests ====================

    #[test]
    fn test_start_scope_returns_enriched_scope() {
        let mut analysis = Analysis::new();

        // Start outer scope
        let outer = analysis.start_scope(make_scope(
            10,
            5,
            NO_PARENT,
            AnalysisScopeType::Loop,
            vec![make_binding("item", 100, 104)],
        ));

        assert_eq!(outer.id, 10);
        assert_eq!(outer.parent_scope_id, NO_PARENT);
        assert_eq!(analysis.scope_stack.len(), 1);

        // Start inner scope
        let inner = analysis.start_scope(make_scope(
            20,
            15,
            5,
            AnalysisScopeType::Loop,
            vec![make_binding("nested", 200, 206)],
        ));

        assert_eq!(inner.id, 20);
        assert_eq!(inner.parent_scope_id, 10);
        assert_eq!(inner.parent_bindings.bindings.len(), 1);
        assert_eq!(inner.parent_bindings.bindings[0].name, "item");
        assert_eq!(analysis.scope_stack.len(), 2);
    }

    // ==================== close_scope Tests ====================

    #[test]
    fn test_close_scope_removes_matching() {
        let mut analysis = Analysis::new();

        // Start a scope
        analysis.start_scope(make_scope(
            10,
            5,
            NO_PARENT,
            AnalysisScopeType::Loop,
            vec![],
        ));

        assert_eq!(analysis.scope_stack.len(), 1);

        // Close the scope
        let closed = analysis.close_scope(5);

        assert_eq!(closed, vec![10]);
        assert_eq!(analysis.scope_stack.len(), 0);
    }

    #[test]
    fn test_close_scope_leaves_unrelated() {
        let mut analysis = Analysis::new();

        // Start two scopes on different elements
        analysis.start_scope(make_scope(
            10,
            5,
            NO_PARENT,
            AnalysisScopeType::Loop,
            vec![],
        ));
        analysis.start_scope(make_scope(20, 15, 5, AnalysisScopeType::Slot, vec![]));

        assert_eq!(analysis.scope_stack.len(), 2);

        // Close only element 15
        let closed = analysis.close_scope(15);

        assert_eq!(closed, vec![20]);
        assert_eq!(analysis.scope_stack.len(), 1);
        assert_eq!(analysis.scope_stack[0].id, 10);
    }

    #[test]
    fn test_close_scope_multiple_on_same_element() {
        let mut analysis = Analysis::new();

        // Start two scopes on the same element (e.g., v-for and v-if)
        analysis.start_scope(make_scope(
            10,
            5,
            NO_PARENT,
            AnalysisScopeType::Loop,
            vec![],
        ));
        analysis.start_scope(make_scope(
            20,
            5,
            NO_PARENT,
            AnalysisScopeType::Conditional,
            vec![],
        ));

        assert_eq!(analysis.scope_stack.len(), 2);

        // Close element 5 - should close both
        let closed = analysis.close_scope(5);

        assert_eq!(closed.len(), 2);
        assert!(closed.contains(&10));
        assert!(closed.contains(&20));
        assert_eq!(analysis.scope_stack.len(), 0);
    }

    // ==================== event Tests ====================

    #[test]
    fn test_event_enriches_with_parent_context() {
        let mut analysis = Analysis::new();

        // Start a parent scope
        analysis.start_scope(make_scope(
            10,
            5,
            NO_PARENT,
            AnalysisScopeType::Loop,
            vec![make_binding("item", 100, 104)],
        ));

        // Create an event data
        let event_data = AnalysisScopeEventData {
            r#type: AnalysisScopeType::DirectiveExp,
            start: 200,
            end: 220,
            parent_id: 5,
            element_id: 15,
            bindings: BindingExtractionResult {
                bindings: vec![make_binding("foo", 200, 203)],
                ..Default::default()
            },
            condition: None,
            parent_scope_id: NO_PARENT,
            parent_bindings: None,
            provided_bindings: None,
            parent_conditions: Vec::new(),
        };

        let enriched = analysis.event(event_data);

        assert_eq!(enriched.parent_scope_id, 10);
        assert!(enriched.parent_bindings.is_some());
        assert_eq!(enriched.parent_bindings.as_ref().unwrap().bindings.len(), 1);
        assert!(enriched.provided_bindings.is_some());
    }

    // ==================== Integration-style Tests ====================

    #[test]
    fn test_nested_v_for_binding_propagation() {
        let mut analysis = Analysis::new();

        // Outer v-for: v-for="item in items"
        let outer = analysis.start_scope(make_scope(
            10,
            5,
            NO_PARENT,
            AnalysisScopeType::Loop,
            vec![make_binding("item", 100, 104)],
        ));
        assert_eq!(outer.parent_bindings.bindings.len(), 0);

        // Inner v-for: v-for="child in item.children"
        let inner = analysis.start_scope(make_scope(
            20,
            15,
            5,
            AnalysisScopeType::Loop,
            vec![make_binding("child", 200, 205)],
        ));
        assert_eq!(inner.parent_bindings.bindings.len(), 1);
        assert_eq!(inner.parent_bindings.bindings[0].name, "item");

        // Expression inside inner loop can see both bindings
        let expr = analysis.event(AnalysisScopeEventData {
            r#type: AnalysisScopeType::DirectiveExp,
            start: 300,
            end: 320,
            parent_id: 15,
            element_id: 25,
            bindings: BindingExtractionResult::default(),
            condition: None,
            parent_scope_id: NO_PARENT,
            parent_bindings: None,
            provided_bindings: None,
            parent_conditions: Vec::new(),
        });

        // Should see both item and child in parent_bindings
        let parent_bindings = expr.parent_bindings.unwrap();
        assert_eq!(parent_bindings.bindings.len(), 2);
    }

    #[test]
    fn test_conditional_chain_tracking() {
        let mut analysis = Analysis::new();

        // v-if="show"
        let mut if_scope = make_scope(10, 5, NO_PARENT, AnalysisScopeType::Conditional, vec![]);
        if_scope.condition = Some(make_condition(OxcVConditionType::If, 0, 20));
        let if_result = analysis.start_scope(if_scope);
        assert!(if_result.condition.as_ref().unwrap().siblings.is_empty());

        // Close the v-if element
        analysis.close_scope(5);

        // v-else-if="other"
        let mut else_if_scope =
            make_scope(20, 15, NO_PARENT, AnalysisScopeType::Conditional, vec![]);
        else_if_scope.condition = Some(make_condition(OxcVConditionType::ElseIf, 30, 50));
        let else_if_result = analysis.start_scope(else_if_scope);

        // v-else-if should see v-if as sibling
        let siblings = &else_if_result.condition.as_ref().unwrap().siblings;
        assert_eq!(siblings.len(), 1);
        assert!(matches!(siblings[0].condition_type, OxcVConditionType::If));
    }

    // ==================== Binding Reference Collection Tests ====================

    #[test]
    fn test_collect_binding_refs_basic() {
        let mut analysis = Analysis::new();

        let bindings = BindingExtractionResult {
            bindings: vec![make_binding("foo", 10, 13), make_binding("bar", 20, 23)],
            ..Default::default()
        };

        analysis.collect_binding_refs(&bindings, 100, BindingRefContext::Interpolation);

        let refs = &analysis.template_usage.binding_refs;
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name_span, Span::new(10, 13));
        assert_eq!(refs[0].element_id, 100);
        assert_eq!(refs[0].context, BindingRefContext::Interpolation);
        assert_eq!(refs[1].name_span, Span::new(20, 23));
    }

    #[test]
    fn test_collect_binding_refs_skips_ignored() {
        let mut analysis = Analysis::new();

        let bindings = BindingExtractionResult {
            bindings: vec![
                make_binding("foo", 10, 13),
                Binding {
                    name: "ignored",
                    span: Span::new(20, 27),
                    pos: 20,
                    ignore: true, // This one should be skipped
                },
            ],
            ..Default::default()
        };

        analysis.collect_binding_refs(&bindings, 100, BindingRefContext::Interpolation);

        let refs = &analysis.template_usage.binding_refs;
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name_span, Span::new(10, 13));
    }

    #[test]
    fn test_collect_binding_refs_tracks_scope() {
        let mut analysis = Analysis::new();

        // Start a scope (e.g., v-for)
        analysis.start_scope(make_scope(
            10,
            5,
            NO_PARENT,
            AnalysisScopeType::Loop,
            vec![make_binding("item", 100, 104)],
        ));

        // Collect bindings while inside the scope
        let bindings = BindingExtractionResult {
            bindings: vec![make_binding("foo", 200, 203)],
            ..Default::default()
        };

        analysis.collect_binding_refs(&bindings, 50, BindingRefContext::DirectiveValue);

        let refs = &analysis.template_usage.binding_refs;
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].scope_id, 10); // Should have the parent scope's ID
    }

    #[test]
    fn test_get_directive_context_event_handler() {
        // v-on:click
        assert_eq!(
            Analysis::get_directive_context(b"v-on:click"),
            BindingRefContext::EventHandler
        );

        // @click shorthand
        assert_eq!(
            Analysis::get_directive_context(b"@click"),
            BindingRefContext::EventHandler
        );

        // v-on (without argument)
        assert_eq!(
            Analysis::get_directive_context(b"v-on"),
            BindingRefContext::EventHandler
        );
    }

    #[test]
    fn test_get_directive_context_other_directives() {
        // v-if
        assert_eq!(
            Analysis::get_directive_context(b"v-if"),
            BindingRefContext::DirectiveValue
        );

        // v-show
        assert_eq!(
            Analysis::get_directive_context(b"v-show"),
            BindingRefContext::DirectiveValue
        );

        // :class (v-bind:class)
        assert_eq!(
            Analysis::get_directive_context(b":class"),
            BindingRefContext::DirectiveValue
        );

        // v-bind
        assert_eq!(
            Analysis::get_directive_context(b"v-bind"),
            BindingRefContext::DirectiveValue
        );

        // v-model
        assert_eq!(
            Analysis::get_directive_context(b"v-model"),
            BindingRefContext::DirectiveValue
        );
    }

    #[test]
    fn test_binding_ref_context_variants() {
        // Test all context variants are distinct
        assert_ne!(
            BindingRefContext::Interpolation,
            BindingRefContext::DirectiveValue
        );
        assert_ne!(
            BindingRefContext::DirectiveValue,
            BindingRefContext::DirectiveArg
        );
        assert_ne!(
            BindingRefContext::DirectiveArg,
            BindingRefContext::EventHandler
        );
    }
}
