//! Source-backed component names and dynamic use checks for the Vue projection.
//!
//! The existing projection plan supplies template uses, script facts supply
//! visible bindings, and TypeScript answers every component type. A missing
//! PascalCase name is a `GlobalComponents` lookup whose absent member is
//! `unknown`, so construction reports an error rather than admitting `any`.

use rustc_hash::{FxHashMap, FxHashSet};
use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ComponentUseId, ExpressionKind, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::component_use::{
    ComponentUseProjection, ComponentUseWitness, MemberValue, TransactionMember,
};
use crate::ide::vue_projection::public_constructor::PUBLIC_COMPONENT;
use crate::ide::vue_projection::script_setup::{ScriptProjectionFacts, SetupStatementKind};
use crate::ide::{matches_custom_element, sanitize_js_identifier, GlobalComponentFallback};
use crate::template::code_gen::shared::helpers::{is_member_expression, to_pascal_case};

/// The type-level correlation check distributes over each branch of an
/// authored choice object. It refuses the entire value if any branch's props
/// cannot be passed to that branch's component. No independent prop union is
/// synthesized. The returned instance is a union of the actual constructors'
/// instance types, so downstream observations still read public signatures.
pub const DYNAMIC_USE_PRELUDE: &str = concat!(
    "type __VerterDynamicProps<C> = C extends abstract new (props: infer P, ...args: any[]) => unknown ? P : C extends (props: infer P, ...args: any[]) => unknown ? P : never;\n",
    "type __VerterDynamicMismatch<C, P> = C extends unknown ? P extends __VerterDynamicProps<C> ? never : C : never;\n",
    "type __VerterDynamicBad<T, CK extends keyof T, PK extends keyof T> = T extends unknown ? [T[CK]] extends [never] ? T : [T[PK]] extends [never] ? T : [__VerterDynamicMismatch<T[CK], T[PK]>] extends [never] ? never : T : never;\n",
    "type __VerterDynamicInstance<T, CK extends keyof T> = T extends unknown ? T[CK] extends abstract new (...args: any[]) => infer I ? I : T[CK] extends (...args: any[]) => infer R ? R : never : never;\n",
    "declare function __VerterDynamicCorrelated<T, CK extends keyof T, PK extends keyof T>(choice: T & ([__VerterDynamicBad<T, CK, PK>] extends [never] ? unknown : never), componentKey: CK, propsKey: PK): __VerterDynamicInstance<T, CK>;\n",
);

/// How the authored component expression is made available to TypeScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ComponentSource {
    /// An exact local or imported value binding.
    Local,
    /// A member path through a locally bound namespace or barrel alias.
    Namespace,
    /// The carrier's own public constructor.
    Recursive,
    /// An augmentation-backed lookup, fail closed when absent.
    Global,
    /// An authored `:is` expression checked in its lexical environment.
    Dynamic,
    /// A member path whose root is not in scope; TypeScript reports it.
    Unresolved,
}

/// Snapshot and resolution qualified identity for one checking transaction.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolutionSpecializationKey(CanonicalDigest);

impl core::fmt::Debug for ResolutionSpecializationKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ResolutionSpecializationKey({})", self.0.to_hex())
    }
}

/// One revision-qualified resolution of a logical component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentResolutionObservation {
    /// Logical use identity.
    pub use_id: ComponentUseId,
    /// The admitted component expression, with source mapping kept by the plan.
    pub authored_expression: AdmittedExpressionId,
    /// Checking-input identity including the plan snapshot and binding route.
    pub specialization: ResolutionSpecializationKey,
    /// Resolution route.
    pub source: ComponentSource,
    /// Value expression in the checking module.
    pub expression: String,
    /// Global lookup name, when the value needs a fallback declaration.
    pub global_name: Option<String>,
    /// The authored spelling for a fallback seen only in non-Pascal form.
    pub authored_non_pascal: Option<String>,
    /// The witness with its resolved component expression.
    pub witness: ComponentUseWitness,
}

/// A dynamic use and any recoverable component/props choice correlation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicComponentUseContract {
    /// Logical use identity.
    pub use_id: ComponentUseId,
    /// Authored component expression.
    pub expression: String,
    /// Shared object of the component property and the props property.
    ///
    /// Set only for one pure spread whose component expression and spread
    /// expression are simple member paths of that same object. The property
    /// names are the authored ones (`view`/`data` as well as `component`/`props`).
    pub correlated_source: Option<String>,
    /// Property of [`Self::correlated_source`] that holds the constructor.
    pub component_key: Option<String>,
    /// Property of [`Self::correlated_source`] that holds that constructor's props.
    pub props_key: Option<String>,
}

impl DynamicComponentUseContract {
    /// Render exactly one check in the use's lexical scope.
    #[must_use]
    pub fn render(&self, witness: &ComponentUseWitness) -> String {
        match (
            &self.correlated_source,
            &self.component_key,
            &self.props_key,
        ) {
            (Some(source), Some(component_key), Some(props_key)) => format!(
                "const {} = __VerterDynamicCorrelated({source}, \"{component_key}\", \"{props_key}\");\n",
                witness.binding
            ),
            _ => witness.render(),
        }
    }
}

/// All resolutions from one exact projection snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentResolutionProjection {
    /// Plan snapshot.
    pub snapshot: PlanSnapshotId,
    /// Incomplete inputs cannot warm a complete cache.
    pub complete: bool,
    /// One observation per component-resolved use, in source order.
    pub observations: Vec<ComponentResolutionObservation>,
    /// Dynamic use contracts in source order.
    pub dynamic: Vec<DynamicComponentUseContract>,
}

impl ComponentResolutionProjection {
    /// Existing global helper imports remain owned by the checking composer.
    /// This renders their declarations and navigation probes once per name.
    #[must_use]
    pub fn render_fallbacks(&self) -> String {
        let mut names: Vec<(&str, Option<&str>)> = Vec::new();
        let mut index: FxHashMap<&str, usize> = FxHashMap::default();
        for observation in &self.observations {
            let Some(name) = observation.global_name.as_deref() else {
                continue;
            };
            if let Some(&slot) = index.get(name) {
                if observation.authored_non_pascal.is_none() {
                    names[slot].1 = None;
                }
            } else {
                index.insert(name, names.len());
                names.push((name, observation.authored_non_pascal.as_deref()));
            }
        }
        let fallbacks = names
            .into_iter()
            .map(|(name, authored)| GlobalComponentFallback {
                pascal: name.to_string(),
                authored_non_pascal: authored.map(str::to_string),
            })
            .collect::<Vec<_>>();
        let mut out = String::new();
        crate::ide::script::emit_global_component_fallbacks(&mut out, &fallbacks, false);
        out
    }

    /// Prelude, fallbacks, then exactly one transaction per use.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::from(crate::ide::vue_projection::component_use::USE_PRELUDE);
        out.push_str(DYNAMIC_USE_PRELUDE);
        out.push_str(&self.render_fallbacks());
        let dynamic: FxHashMap<&ComponentUseId, &DynamicComponentUseContract> = self
            .dynamic
            .iter()
            .map(|use_| (&use_.use_id, use_))
            .collect();
        for observation in &self.observations {
            if let Some(dynamic) = dynamic.get(&observation.use_id) {
                out.push_str(&dynamic.render(&observation.witness));
            } else {
                out.push_str(&observation.witness.render());
            }
        }
        out
    }
}

/// Resolve names using the already-owned plan and script binding inventory.
/// `canonical_id` is used solely for Vue's filename-based self reference.
#[must_use]
pub fn project_component_resolution(
    plan: &ProjectionPlan,
    uses: &ComponentUseProjection,
    script: &ScriptProjectionFacts,
    canonical_id: &str,
    custom_elements: Option<&[String]>,
) -> ComponentResolutionProjection {
    let self_name = sanitize_js_identifier(canonical_id);
    let mut known: FxHashSet<&str> = script
        .module
        .normal_script_bindings
        .iter()
        .map(String::as_str)
        .collect();
    if let Some(setup) = &script.setup {
        for statement in &setup.statements {
            match &statement.kind {
                SetupStatementKind::Import { names }
                | SetupStatementKind::Declaration { names } => {
                    known.extend(names.iter().map(String::as_str));
                }
                _ => {}
            }
        }
    }
    let mut complete = plan.is_complete() && uses.complete;
    let mut observations = Vec::with_capacity(plan.uses.len());
    let mut dynamic = Vec::new();
    let witnesses: FxHashMap<&ComponentUseId, &ComponentUseWitness> = uses
        .witnesses
        .iter()
        .map(|witness| (&witness.use_id, witness))
        .collect();
    for use_ in &plan.uses {
        let Some(occurrence) = plan.expression(&use_.component_expression) else {
            complete = false;
            continue;
        };
        if occurrence.kind == ExpressionKind::ComponentTag
            && matches_custom_element(custom_elements, occurrence.spelling.trim())
        {
            continue;
        }
        let Some(&original) = witnesses.get(&use_.id) else {
            complete = false;
            continue;
        };
        let spelling = occurrence.spelling.trim();
        let (source, expression, global_name, authored_non_pascal) =
            if occurrence.kind == ExpressionKind::ComponentIs {
                (
                    ComponentSource::Dynamic,
                    original.component.clone(),
                    None,
                    None,
                )
            } else {
                let name = if spelling.contains('-') {
                    to_pascal_case(spelling)
                } else {
                    spelling.to_string()
                };
                if name.contains('.') {
                    let root = name.split('.').next().unwrap_or("");
                    if known.contains(root) {
                        (ComponentSource::Namespace, name, None, None)
                    } else {
                        (ComponentSource::Unresolved, name, None, None)
                    }
                } else if known.contains(name.as_str()) {
                    (ComponentSource::Local, name, None, None)
                } else if name == self_name {
                    (
                        ComponentSource::Recursive,
                        PUBLIC_COMPONENT.to_string(),
                        None,
                        None,
                    )
                } else if is_member_expression(&name) {
                    let authored = (spelling != name).then(|| spelling.to_string());
                    (ComponentSource::Global, name.clone(), Some(name), authored)
                } else {
                    complete = false;
                    continue;
                }
            };
        let mut witness = original.clone();
        witness.component = expression.clone();
        if source == ComponentSource::Dynamic {
            let choice = correlated_choice(spelling, &witness);
            dynamic.push(DynamicComponentUseContract {
                use_id: use_.id.clone(),
                expression: spelling.to_string(),
                correlated_source: choice.as_ref().map(|choice| choice.source.clone()),
                component_key: choice.as_ref().map(|choice| choice.component_key.clone()),
                props_key: choice.as_ref().map(|choice| choice.props_key.clone()),
            });
        }
        observations.push(ComponentResolutionObservation {
            use_id: use_.id.clone(),
            authored_expression: use_.component_expression.clone(),
            specialization: resolution_specialization(plan, use_, source, &expression, &witness),
            source,
            expression,
            global_name,
            authored_non_pascal,
            witness,
        });
    }
    ComponentResolutionProjection {
        snapshot: plan.snapshot.clone(),
        complete,
        observations,
        dynamic,
    }
}

fn resolution_specialization(
    plan: &ProjectionPlan,
    use_: &crate::framework_common::projection_plan::ComponentUse,
    source: ComponentSource,
    expression: &str,
    witness: &ComponentUseWitness,
) -> ResolutionSpecializationKey {
    let mut encoder = CanonicalEncoder::new(
        "verter.compiler.vue_projection.component_resolution_specialization.v1",
    );
    encoder.field_bytes(1, plan.snapshot.canonical_bytes());
    encoder.field_bytes(2, use_.id.canonical_bytes());
    encoder.field_u32(3, source as u32);
    encoder.field_str(4, expression);
    encoder.field_bytes(5, witness.specialization.digest_bytes());
    ResolutionSpecializationKey(encoder.digest())
}

struct CorrelatedChoice {
    source: String,
    component_key: String,
    props_key: String,
}

/// One pure `v-bind` of a sibling property on the same simple member object
/// as the `:is` expression. Any other shape stays on the uncorrelated
/// construction, which does not claim a finite-union correlation.
fn correlated_choice(component: &str, witness: &ComponentUseWitness) -> Option<CorrelatedChoice> {
    if !witness.transaction.validations.is_empty() || witness.transaction.members.len() != 1 {
        return None;
    }
    let TransactionMember::Spread {
        value: MemberValue::Expression { spelling, .. },
        ..
    } = &witness.transaction.members[0]
    else {
        return None;
    };
    let (component_base, component_key) = split_member_key(component)?;
    let (props_base, props_key) = split_member_key(spelling)?;
    if component_base != props_base {
        return None;
    }
    Some(CorrelatedChoice {
        source: component_base.to_string(),
        component_key: component_key.to_string(),
        props_key: props_key.to_string(),
    })
}

/// Base and final property of a simple member path (`choice.view`, `a.b.data`).
fn split_member_key(expr: &str) -> Option<(&str, &str)> {
    let expr = expr.trim();
    if !is_simple_member(expr) {
        return None;
    }
    let (base, key) = expr.rsplit_once('.')?;
    if !is_simple_member(base) || !is_property_name(key) {
        return None;
    }
    Some((base, key))
}

fn is_simple_member(expr: &str) -> bool {
    is_member_expression(expr) && expr.split('.').all(is_property_name)
}

fn is_property_name(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}
