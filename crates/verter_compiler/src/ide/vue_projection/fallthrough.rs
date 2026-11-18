//! Root shape and the attributes a caller may pass through.

use super::attribute_operations::{AttributeOperationsProjection, ConsumerChannel};
use super::branches::{TemplateItem, TemplateRootNode};
use super::component_resolution::ComponentSource;
use super::component_use::quote;
use super::native_elements::NATIVE_ELEMENT_PRELUDE;
use crate::framework_common::projection_plan::{
    ElementNamespace, ElementUseKind, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::branches::TemplateBranchProjection;
use crate::ide::vue_projection::builtins::BuiltinProjection;
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, TemplateBlockId, UseHost,
};
use crate::ide::vue_projection::component_resolution::ComponentResolutionProjection;
use crate::ide::vue_projection::public_constructor::{
    PublicDeclarationExtras, VuePublicConstructorContract,
};
use crate::template::topology::VueTemplateNodeKind;
use rustc_hash::FxHashMap;

/// Only type-level operations: TypeScript distributes over the root alternatives
/// and subtracts declared props/events before intersecting the caller surface.
pub const FALLTHROUGH_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "50-fallthrough",
    text: concat!(
        "declare const __VerterRootProbe: boolean;\n",
        "declare function __VerterRootArm<W>(w: W): { readonly __verterRootArm: W };\n",
        "type __VerterFallthroughKeys<D> = D extends unknown ? keyof D : never;\n",
        "type __VerterFallthrough<R, D> = R extends unknown ? Partial<Omit<R, Exclude<__VerterFallthroughKeys<D>, 'class' | 'style'>>> : never;\n",
    ),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootShape {
    None,
    Single,
    Multiple,
    Conditional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootEntry {
    pub block: TemplateBlockId,
    pub host: UseHost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootContractSummary {
    pub shape: RootShape,
    pub roots: Vec<RootEntry>,
    pub inherit_attrs: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedFallthroughContract {
    /// Type text intersected into the caller's props; `None` when nothing falls through.
    pub props_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrsReadContract {
    /// Type text of `$attrs` / `useAttrs()`.
    pub attrs_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallthroughProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub roots: RootContractSummary,
    pub fallthrough: TypedFallthroughContract,
    pub attrs: AttrsReadContract,
    root_statements: Vec<PlacedStatement>,
}

impl FallthroughProjection {
    /// The product before the root contract is projected: no root, nothing
    /// falls through, and no `$attrs` type text.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            roots: RootContractSummary {
                shape: RootShape::None,
                roots: Vec::new(),
                inherit_attrs: None,
            },
            fallthrough: TypedFallthroughContract { props_type: None },
            root_statements: Vec::new(),
            attrs: AttrsReadContract {
                attrs_type: String::new(),
            },
        }
    }
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.pending || !self.complete {
            Vec::new()
        } else {
            vec![NATIVE_ELEMENT_PRELUDE, FALLTHROUGH_PRELUDE]
        }
    }
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        if self.pending || !self.complete {
            Vec::new()
        } else {
            self.root_statements.clone()
        }
    }
    #[must_use]
    pub fn declaration_extras(&self) -> PublicDeclarationExtras {
        if self.pending || !self.complete {
            return PublicDeclarationExtras::default();
        }
        PublicDeclarationExtras {
            props_intersection: self.fallthrough.props_type.clone(),
            attrs_type: Some(self.attrs.attrs_type.clone()),
        }
    }
}

/// Collapse only retained root syntax. No authored tree or type is revisited.
struct RootAlternatives {
    shape: RootShape,
    arms: Vec<Option<usize>>,
    hosts: Vec<UseHost>,
}

fn root_children(nodes: &[TemplateRootNode], children: &[usize]) -> RootAlternatives {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut chains: FxHashMap<u32, usize> = FxHashMap::default();
    for &index in children {
        let node = &nodes[index];
        if node.kind == VueTemplateNodeKind::Comment {
            continue;
        }
        if let Some((chain, _)) = node.branch {
            if let Some(&group) = chains.get(&chain) {
                groups[group].push(index);
                continue;
            }
            chains.insert(chain, groups.len());
        }
        groups.push(vec![index]);
    }
    let mut projected = Vec::new();
    for group in &groups {
        let conditional = nodes[group[0]].branch.is_some();
        let mut arms = Vec::new();
        let mut hosts = Vec::new();
        let mut shape = RootShape::None;
        for &index in group {
            let node = &nodes[index];
            let part = if node.repeated {
                RootAlternatives {
                    shape: RootShape::Multiple,
                    arms: vec![None],
                    hosts: node.host.clone().into_iter().collect(),
                }
            } else if node.kind == VueTemplateNodeKind::Template {
                root_children(nodes, &node.children)
            } else {
                RootAlternatives {
                    shape: RootShape::Single,
                    arms: vec![Some(index)],
                    hosts: node.host.clone().into_iter().collect(),
                }
            };
            shape = part.shape;
            arms.extend(part.arms);
            hosts.extend(part.hosts);
        }
        projected.push(RootAlternatives {
            shape: if conditional {
                RootShape::Conditional
            } else {
                shape
            },
            arms,
            hosts,
        });
    }
    match projected.len() {
        0 => RootAlternatives {
            shape: RootShape::None,
            arms: vec![None],
            hosts: Vec::new(),
        },
        1 => projected.pop().expect("one group"),
        _ => RootAlternatives {
            shape: RootShape::Multiple,
            arms: vec![None],
            hosts: projected.into_iter().flat_map(|part| part.hosts).collect(),
        },
    }
}

fn host_item(host: &UseHost) -> TemplateItem {
    match host {
        UseHost::Component(id) => TemplateItem::Component(id.clone()),
        UseHost::Element(id) => TemplateItem::Element(id.clone()),
    }
}

fn root_type(
    plan: &ProjectionPlan,
    node: &TemplateRootNode,
    resolution: &ComponentResolutionProjection,
    builtins: &BuiltinProjection,
    attributes: &AttributeOperationsProjection,
) -> Option<String> {
    if let Some((tag, namespace)) = &node.native {
        if node.host.is_none() {
            let namespace = match namespace {
                ElementNamespace::Html => "html",
                ElementNamespace::Svg => "svg",
                ElementNamespace::MathMl => "mathml",
            };
            return Some(format!(
                "__VerterNativeAttributes<{}, {}>",
                quote(tag),
                quote(namespace)
            ));
        }
    }
    let (raw, keys) = match node.host.as_ref()? {
        UseHost::Element(id) => {
            let element = plan.elements.iter().find(|element| element.id == *id)?;
            let (tag, namespace) = match &element.kind {
                ElementUseKind::Native { tag, namespace } => (
                    tag,
                    match namespace {
                        ElementNamespace::Html => "html",
                        ElementNamespace::Svg => "svg",
                        ElementNamespace::MathMl => "mathml",
                    },
                ),
                ElementUseKind::CustomElement { tag } => (tag, "html"),
                ElementUseKind::SlotOutlet | ElementUseKind::TemplateWrapper => return None,
            };
            (
                format!(
                    "__VerterNativeAttributes<{}, {}>",
                    quote(tag),
                    quote(namespace)
                ),
                attributes
                    .element_key_plans
                    .iter()
                    .find(|keys| keys.use_id == *id)?
                    .effective
                    .iter()
                    .filter(|property| !matches!(property.key.as_str(), "class" | "style"))
                    .map(|property| format!("__VerterNativeKey<{}>", quote(&property.key)))
                    .collect::<Vec<_>>(),
            )
        }
        UseHost::Component(id) => {
            if builtins
                .obligations
                .iter()
                .any(|builtin| builtin.use_id == *id)
            {
                return None;
            }
            let observation = resolution
                .observations
                .iter()
                .find(|observation| observation.use_id == *id)?;
            if matches!(
                observation.source,
                ComponentSource::Dynamic | ComponentSource::Recursive | ComponentSource::Unresolved
            ) {
                return None;
            }
            let key_plan = attributes
                .key_plans
                .iter()
                .find(|keys| keys.use_id == *id)?;
            let mut keys = Vec::new();
            for property in &key_plan.effective {
                if matches!(property.key.as_str(), "class" | "style") {
                    continue;
                }
                for channel in &attributes.relation(id, &property.key)?.channels {
                    let key = match channel {
                        ConsumerChannel::DeclaredProp { name } => name,
                        ConsumerChannel::FallthroughAttr { key } => key,
                        ConsumerChannel::EmittedEvent { .. } | ConsumerChannel::Reserved => {
                            continue
                        }
                    };
                    let key = quote(key);
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
            }
            (
                format!("typeof {}[\"$props\"]", observation.witness.binding),
                keys,
            )
        }
    };
    let consumed = keys.join(" | ");
    Some(if keys.is_empty() {
        raw
    } else if matches!(node.host, Some(UseHost::Component(_))) {
        format!("__VerterUseDrop<{raw}, {consumed}>")
    } else {
        format!("Omit<{raw}, Exclude<{consumed}, \"class\" | \"style\">>")
    })
}

#[must_use]
pub fn project_fallthrough(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    resolution: &ComponentResolutionProjection,
    public: &VuePublicConstructorContract,
    builtins: &BuiltinProjection,
    attributes: &AttributeOperationsProjection,
) -> FallthroughProjection {
    let mut projection = FallthroughProjection::pending(plan.snapshot.clone());
    projection.pending = false;
    projection.roots.inherit_attrs = public.options.inherit_attrs;
    projection.complete = plan.is_complete()
        && branches.complete
        && resolution.complete
        && attributes.complete
        && builtins.complete
        && !builtins.pending
        && branches.snapshot == plan.snapshot
        && resolution.snapshot == plan.snapshot
        && attributes.snapshot == plan.snapshot
        && builtins.snapshot == plan.snapshot;
    let nodes = branches.runtime_roots();
    let Some(root) = nodes.first() else {
        projection.complete = false;
        return projection;
    };
    let roots = root_children(nodes, &root.children);
    projection.roots.shape = roots.shape;
    for host in roots.hosts {
        if let Some((block, _)) = branches.place_of(&host_item(&host)) {
            projection.roots.roots.push(RootEntry { block, host });
        } else {
            projection.complete = false;
        }
    }
    // Component root arms return their existing specialized witness through
    // the setup scope. No imported generic-status fact or bare constructor
    // signature is needed. Fragments contribute no component arm.
    let has_component_root = roots
        .arms
        .iter()
        .flatten()
        .any(|&index| nodes[index].kind == VueTemplateNodeKind::Component);
    let mut returned = format!(
        "ReturnType<typeof __VerterSetupScope{}>",
        public.binder_args()
    );
    if public.setup_is_async() {
        returned = format!("Awaited<{returned}>");
    }
    let bridge = format!(
        "Extract<{returned}, {{ readonly __verterRootArm: unknown }}>[\"__verterRootArm\"][\"$props\"]"
    );
    let declared = public.declared_props_type();
    let mut arms = Vec::new();
    let mut has_surface = false;
    for index in roots.arms {
        let mut raw = index
            .and_then(|index| root_type(plan, &nodes[index], resolution, builtins, attributes));
        if let Some(index) = index {
            if let Some(UseHost::Component(id)) = &nodes[index].host {
                if let Some(props) = &raw {
                    if let (Some(observation), Some((block, item))) = (
                        resolution.observations.iter().find(|o| o.use_id == *id),
                        branches.place_of(&TemplateItem::Component(id.clone())),
                    ) {
                        // Subtract this arm's consumed keys while its witness
                        // is in scope, before TypeScript unions the returns.
                        projection.root_statements.push(PlacedStatement {
                            anchor: StatementAnchor::Item { block, item },
                            phase: StatementPhase::After,
                            family: ObligationFamily::Fallthrough,
                            host: Some(UseHost::Component(id.clone())),
                            fragment: CheckingFragment::synth(format!(
                                "if (__VerterRootProbe) return __VerterRootArm<{{ readonly $props: {props} }}>({});\n",
                                observation.witness.binding
                            )),
                        });
                        raw = Some(bridge.clone());
                    } else {
                        raw = None;
                    }
                }
            }
        }
        if raw.is_none()
            && (projection.roots.shape == RootShape::Conditional
                || index.is_some_and(|index| {
                    matches!(
                        nodes[index].kind,
                        VueTemplateNodeKind::Component | VueTemplateNodeKind::SlotOutlet
                    )
                }))
        {
            projection.complete = false;
        }
        has_surface |= raw.is_some();
        let arm = raw.unwrap_or_else(|| {
            "Pick<import(\"vue\").HTMLAttributes, \"class\" | \"style\">".to_string()
        });
        if !arms.contains(&arm) {
            arms.push(arm);
        }
    }
    let raw = arms.join(" | ");
    let attrs = format!("__VerterFallthrough<({raw}), ({declared})>");
    // Reading the bridge from $attrs would feed the inferred setup return
    // back into its own input (for example a v-bind="$attrs" wrapper).
    projection.attrs.attrs_type = if has_component_root {
        "Record<string, unknown>".to_string()
    } else {
        attrs.clone()
    };
    if public.options.inherit_attrs != Some(false)
        && matches!(
            projection.roots.shape,
            RootShape::Single | RootShape::Conditional
        )
        && has_surface
    {
        projection.fallthrough.props_type = Some(attrs);
    }
    projection
}
