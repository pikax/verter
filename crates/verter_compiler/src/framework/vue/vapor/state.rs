//! The immutable runs, operation rows, and two outputs of one fine-grained render.
use oxc_allocator::Allocator;
use rustc_hash::{FxHashMap, FxHashSet};

use super::counters::VaporCounters;
use super::effect::{EffectGroupId, EffectId, VaporEffect};
use super::graph::EffectGraph;
use super::ops::{ClosedBlock, Runs};
use crate::assembly::emit_plan::{EmitPlan, EmitPlanBuilder, EmitPlanError, MapMode};
use crate::emission::{EmissionBuilder, EmissionError, QualifiedSource};
use crate::framework::vue::target::VueTargetRefusal;
use crate::template::code_gen::shared::helpers::{VaporHelper, VaporHelperFlags};

/// A template's index is minted when its scope closes, after its nested blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TemplateDeclaration {
    pub(super) index: u32,
    pub(super) html: String,
    pub(super) is_root: bool,
    pub(super) is_static: bool,
    /// The namespace runtime-vapor parses the payload in: 0 for HTML, 1 for
    /// SVG, 2 for MathML (Vue's `Namespaces`).
    pub(super) namespace: u8,
}

/// A component the render function resolves by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComponentAsset<'c> {
    /// The local holding the resolved component.
    pub(super) name: &'c str,
    /// The tag as a quoted JavaScript string.
    pub(super) quoted_tag: &'c str,
}

/// Builders remain unwritten until the shared walk leaves the template.
pub(super) struct VaporState<'c> {
    source_text: &'c str,
    pub(super) render: EmissionBuilder<'c>,
    pub(super) preamble: EmissionBuilder<'c>,
    pub(super) counters: VaporCounters,
    pub(super) templates: Vec<TemplateDeclaration>,
    pub(super) runs: Runs<'c>,
    pub(super) effects: Vec<VaporEffect<'c>>,
    pub(super) effect_graph: EffectGraph<'c>,
    pub(super) roots: Vec<ClosedBlock>,
    pub(super) delegated_events: FxHashSet<String>,
    /// Components resolved by name, once each, in first-use order.
    pub(super) component_assets: Vec<ComponentAsset<'c>>,
    /// Hoisted event handler locals already declared in this render.
    handler_names: FxHashSet<String>,
    /// A template ref inside a loop calls the render's own ref setter.
    pub(super) template_ref_setter: bool,
    /// Dynamic slot function locals declared so far.
    slot_caches: u32,
    /// Slot-root metadata of each construct, by its node name, until a slot
    /// block marks it or the render ends.
    pub(super) slot_root_marks: FxHashMap<u32, super::slot_roots::SlotRootMark>,
    /// The roots of each block returning several, by its run.
    pub(super) block_roots: FxHashMap<u32, Vec<super::slot_roots::SlotRoot>>,
    helpers: VaporHelperFlags,
    pub(super) builtins: crate::template::code_gen::shared::helpers::BuiltinComponentFlags,
}

impl<'c> VaporState<'c> {
    pub(super) fn new(source: QualifiedSource<'c>, mode: MapMode) -> Self {
        let source_text = source.text;
        Self {
            source_text,
            render: EmissionBuilder::new(mode).with_source(source.clone()),
            preamble: EmissionBuilder::new(mode).with_source(source),
            counters: VaporCounters::default(),
            templates: Vec::new(),
            runs: Runs::default(),
            effects: Vec::new(),
            effect_graph: EffectGraph::default(),
            roots: Vec::new(),
            delegated_events: FxHashSet::default(),
            component_assets: Vec::new(),
            handler_names: FxHashSet::default(),
            template_ref_setter: false,
            slot_caches: 0,
            slot_root_marks: FxHashMap::default(),
            block_roots: FxHashMap::default(),
            helpers: VaporHelperFlags::empty(),
            builtins: crate::template::code_gen::shared::helpers::BuiltinComponentFlags::empty(),
        }
    }

    pub(super) fn use_helper(&mut self, helper: VaporHelper) {
        self.helpers = self.helpers.add(helper);
    }

    pub(super) fn helpers(&self) -> VaporHelperFlags {
        self.helpers
    }

    /// Declare `const _setTemplateRef = _createTemplateRefSetter()` at the
    /// top of `render`.
    pub(super) fn declare_template_ref_setter(&mut self) {
        self.template_ref_setter = true;
        self.use_helper(VaporHelper::CreateTemplateRefSetter);
    }

    /// A render-unique local that keeps one dynamic slot's function.
    pub(super) fn slot_cache_name(&mut self, allocator: &'c Allocator) -> &'c str {
        let mut name = String::from("_slot");
        crate::template::code_gen::shared::helpers::push_u32(&mut name, self.slot_caches);
        self.slot_caches += 1;
        allocator.alloc_str(&name)
    }

    pub(super) fn delegate_event(&mut self, name: &str) {
        if !self.delegated_events.contains(name) {
            self.delegated_events.insert(name.to_owned());
            self.use_helper(VaporHelper::DelegateEvents);
        }
    }

    /// The local holding the component the render function resolves for
    /// `quoted_tag`. The first use of a tag declares it and later uses share
    /// it; a different tag whose mangled `base` is already taken (`Foo.Bar`
    /// beside `Foo-Bar`) gets `base1`, `base2`, ...
    pub(super) fn resolve_component(
        &mut self,
        base: &str,
        quoted_tag: &'c str,
        allocator: &'c Allocator,
    ) -> &'c str {
        self.use_helper(VaporHelper::ResolveComponent);
        if let Some(asset) = self
            .component_assets
            .iter()
            .find(|asset| asset.quoted_tag == quoted_tag)
        {
            return asset.name;
        }
        let mut name = base.to_owned();
        let mut suffix = 0;
        while self.component_assets.iter().any(|asset| asset.name == name) {
            suffix += 1;
            name.truncate(base.len());
            crate::template::code_gen::shared::helpers::push_u32(&mut name, suffix);
        }
        let name = allocator.alloc_str(&name);
        self.component_assets
            .push(ComponentAsset { name, quoted_tag });
        name
    }

    /// A render-unique local for a hoisted handler of `event`:
    /// `_on_<event>`, then `_on_<event>1`, `_on_<event>2`, ... skipping
    /// every name `unavailable` reports as visible where the local is declared.
    pub(super) fn handler_name(
        &mut self,
        event: &str,
        unavailable: impl Fn(&str) -> bool,
    ) -> String {
        let mut name = String::from("_on_");
        name.extend(event.chars().map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        }));
        let base = name.len();
        let mut suffix = 0;
        while self.handler_names.contains(&name) || unavailable(&name) {
            suffix += 1;
            name.truncate(base);
            crate::template::code_gen::shared::helpers::push_u32(&mut name, suffix);
        }
        self.handler_names.insert(name.clone());
        name
    }

    pub(super) fn declare_template(
        &mut self,
        html: String,
        is_root: bool,
        is_static: bool,
        namespace: u8,
    ) -> u32 {
        let index = self.counters.next_template();
        self.use_helper(VaporHelper::Template);
        self.templates.push(TemplateDeclaration {
            index,
            html,
            is_root,
            is_static,
            namespace,
        });
        index
    }

    /// Vue 3.6.0-rc.15 `isStableTemplateSlotRoot`: a slot root's template is
    /// stable unless it is empty or holds only comments, which render nothing
    /// a fallback could replace.
    pub(super) fn is_stable_template_root(&self, template: Option<u32>) -> bool {
        let Some(template) = template else {
            return false;
        };
        self.templates
            .iter()
            .find(|declaration| declaration.index == template)
            .is_some_and(|declaration| {
                // Vue tests the trimmed content against
                // `/^(?:<!--[\s\S]*?-->)+$/`, which any content between a
                // leading `<!--` and a trailing `-->` matches.
                let trimmed = declaration.html.trim();
                let comment_only =
                    trimmed.len() >= 7 && trimmed.starts_with("<!--") && trimmed.ends_with("-->");
                !declaration.html.is_empty() && !comment_only
            })
    }

    pub(super) fn add_effect(&mut self, effect: VaporEffect<'c>) -> EffectId {
        // A text effect writes `_setText` wherever its node is declared.
        if matches!(effect, VaporEffect::Text { .. }) {
            self.use_helper(VaporHelper::SetText);
        }
        let id = EffectId(self.effects.len() as u32);
        self.effects.push(effect);
        id
    }

    pub(super) fn add_effect_group(
        &mut self,
        effects: Vec<EffectId>,
        indent: &'c str,
        wrapper: super::graph::EffectWrapper,
    ) -> Result<EffectGroupId, VueTargetRefusal> {
        let group = self.effect_graph.add_group(
            effects,
            indent,
            wrapper,
            &self.effects,
            &self.runs,
            self.source_text,
        )?;
        self.use_helper(VaporHelper::RenderEffect);
        if matches!(wrapper, super::graph::EffectWrapper::Memo { .. }) {
            self.use_helper(VaporHelper::WithMemo);
        }
        Ok(group)
    }

    /// Close the two builders into one-fragment plans; an empty preamble is absent.
    pub(super) fn finish(self) -> Result<(EmitPlan<'c>, Option<EmitPlan<'c>>), VaporFinishError> {
        let render = single_fragment_plan(self.render)?;
        let preamble = if self.preamble.is_empty() {
            None
        } else {
            Some(single_fragment_plan(self.preamble)?)
        };
        Ok((render, preamble))
    }
}

fn single_fragment_plan(builder: EmissionBuilder<'_>) -> Result<EmitPlan<'_>, VaporFinishError> {
    let mode = builder.mode();
    let fragment = builder.finish().map_err(VaporFinishError::Emission)?;
    let mut plan = EmitPlanBuilder::new(mode);
    plan.push_fragment(fragment)
        .map_err(VaporFinishError::Plan)?;
    plan.finish().map_err(VaporFinishError::Plan)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum VaporFinishError {
    Emission(EmissionError),
    Plan(EmitPlanError),
}
