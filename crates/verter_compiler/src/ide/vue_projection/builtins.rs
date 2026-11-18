#![allow(dead_code)]
//! Vue built-in components.

use crate::framework_common::projection_plan::{
    ComponentUseId, ExpressionKind, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, PlacedStatement, PreludeDeclaration,
};
use crate::ide::vue_projection::component_resolution::ComponentResolutionProjection;
use crate::template::vue_syntax::builtin_component;
pub use crate::template::vue_syntax::BuiltinComponent;
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinComponentObligation {
    pub use_id: ComponentUseId,
    pub builtin: BuiltinComponent,
    /// Constructor expression checked instead of a setup binding.
    pub constructor: String,
}

impl BuiltinComponentObligation {
    fn binding(&self) -> String {
        format!("__VerterBuiltin_{}", &self.use_id.digest_hex()[..16])
    }

    /// The shared callable/constructor adapter supplies the instance. Its
    /// published-props primitive then retains that instance's slots and refs.
    #[must_use]
    pub fn before(&self) -> CheckingFragment {
        CheckingFragment::synth(format!(
            "const {} = __VerterUseConstructor(null! as typeof import(\"vue\").{});\n",
            self.binding(),
            self.builtin.export_name(),
        ))
    }
}

/// Which carrier shapes the module is composed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierShape {
    ScriptSetup,
    OptionsOnly,
    ScriptAndSetup,
    TemplateOnly,
    Empty,
}

impl CarrierShape {
    /// Classify parser-owned block presence, including empty script blocks.
    #[must_use]
    pub fn of(parsed: &crate::parser::types::ParsedSfc) -> Self {
        match (parsed.script().is_some(), parsed.script_setup().is_some()) {
            (true, true) => Self::ScriptAndSetup,
            (false, true) => Self::ScriptSetup,
            (true, false) => Self::OptionsOnly,
            (false, false) if parsed.template_ast().is_some() => Self::TemplateOnly,
            (false, false) => Self::Empty,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateFeatureParticipation {
    pub shape: CarrierShape,
    pub builtins: Vec<ComponentUseId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub obligations: Vec<BuiltinComponentObligation>,
    pub participation: TemplateFeatureParticipation,
}

impl BuiltinProjection {
    /// The product before built-ins are checked: it contributes nothing and
    /// records no carrier shape beyond `Empty`.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            obligations: Vec::new(),
            participation: TemplateFeatureParticipation {
                shape: CarrierShape::Empty,
                builtins: Vec::new(),
            },
        }
    }
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        // Both normalization steps are already declared by the use prelude.
        Vec::new()
    }
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        // The composer places `before` at the existing component-use anchor.
        Vec::new()
    }

    /// Keep ordinary component fallbacks, excluding builtin uses whose
    /// constructors come directly from Vue's published surface.
    #[must_use]
    pub fn render_fallbacks(&self, resolution: &ComponentResolutionProjection) -> String {
        let builtin_ids: FxHashSet<_> = self.obligations.iter().map(|o| &o.use_id).collect();
        let mut ordinary = resolution.clone();
        ordinary
            .observations
            .retain(|o| !builtin_ids.contains(&o.use_id));
        ordinary.render_fallbacks()
    }
}

#[must_use]
pub fn project_builtins(
    plan: &ProjectionPlan,
    resolution: &ComponentResolutionProjection,
    shape: CarrierShape,
) -> BuiltinProjection {
    let mut obligations = Vec::new();
    for observation in &resolution.observations {
        let Some(expression) = plan.expression(&observation.authored_expression) else {
            continue;
        };
        if expression.kind != ExpressionKind::ComponentTag {
            continue;
        }
        let Some(builtin) = builtin_component(expression.spelling.trim()) else {
            continue;
        };
        let mut obligation = BuiltinComponentObligation {
            use_id: observation.use_id.clone(),
            builtin,
            constructor: String::new(),
        };
        obligation.constructor = format!(
            "(null! as __VerterUseConstruct<any[], InstanceType<typeof {}>>)",
            obligation.binding(),
        );
        obligations.push(obligation);
    }
    BuiltinProjection {
        snapshot: plan.snapshot.clone(),
        complete: plan.is_complete() && resolution.complete && resolution.snapshot == plan.snapshot,
        pending: false,
        participation: TemplateFeatureParticipation {
            shape,
            builtins: obligations.iter().map(|o| o.use_id.clone()).collect(),
        },
        obligations,
    }
}
