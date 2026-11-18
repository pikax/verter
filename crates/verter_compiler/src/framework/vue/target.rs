#![allow(dead_code, unused_variables)]
//! What every Vue target reads and returns.

use oxc_allocator::Allocator;
use rustc_hash::FxHashSet;
use verter_span::Span;

use super::plan::VueTarget;
use super::semantic::VueSemanticSnapshot;
use super::structure::VueCompileStructure;
use super::style::VueComponentScopePlan;
use crate::assembly::emit_plan::{EmitPlan, MapMode};
use crate::emission::QualifiedSource;
use crate::framework_common::{RuntimeDiagnosticSeverity, TemplateRenderExport};
use crate::template::topology::VueTemplateTopology;

/// Resolved options of one target run.
#[derive(Debug, Clone)]
pub struct VueTargetOptions {
    pub inline: bool,
    pub is_production: bool,
    pub comments: bool,
    /// Strip TypeScript syntax from template expressions.
    pub force_js: bool,
    /// PascalCase name of the component's own file, for self-reference.
    pub self_name: String,
    pub has_script: bool,
    pub hoist_static: bool,
    pub cache_handlers: bool,
    /// Props proven constant across call sites, when the host supplied them.
    pub const_props: Option<FxHashSet<String>>,
    /// Imports a template `ref="name"` may bind in inline mode.
    pub ref_bindable_imports: FxHashSet<String>,
    pub map_mode: MapMode,
}

/// Shared prerequisites of every target, all borrowed for one compile.
pub struct VueTargetInputs<'c> {
    /// The SFC source with its identity. Every template offset is absolute
    /// in `source.text`, and the render fragment names this source.
    pub source: QualifiedSource<'c>,
    /// Arena of the edit recorder and the render transform; the render
    /// fragment borrows it, so it lives as long as the inputs.
    pub allocator: &'c Allocator,
    pub topology: &'c VueTemplateTopology<'c>,
    pub structure: &'c VueCompileStructure,
    pub semantic: &'c VueSemanticSnapshot<'c>,
    pub scope: &'c VueComponentScopePlan,
    pub options: &'c VueTargetOptions,
}

/// Runtime helpers a render function imports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VueHelperSet {
    /// From the runtime module (`vue` by default), in emission order.
    pub runtime: Vec<&'static str>,
    /// From the server renderer module.
    pub server: Vec<&'static str>,
}

/// A diagnostic a target raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueTargetDiagnostic {
    pub severity: RuntimeDiagnosticSeverity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

/// The render function of one target, as a plan over the SFC source.
pub struct VueTargetOutput<'c> {
    pub target: VueTarget,
    /// The render function (or inline render closure) text.
    pub render: EmitPlan<'c>,
    /// Module-level declarations the render function needs (hoisted
    /// constants, template declarations), emitted before the component.
    pub preamble: Option<EmitPlan<'c>>,
    pub helpers: VueHelperSet,
    pub render_export: TemplateRenderExport,
    pub diagnostics: Vec<VueTargetDiagnostic>,
}

/// A target cannot produce output for this template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueTargetRefusal {
    /// The construct at `span` has no supported lowering on this target.
    Unsupported { code: &'static str, span: Span },
    /// The plan could not be linearized.
    Emit(crate::assembly::emit_plan::EmitPlanError),
    /// The transform refused a recorded edit; the recording site is wrong.
    Edit(crate::code_transform::CodeTransformError),
}
