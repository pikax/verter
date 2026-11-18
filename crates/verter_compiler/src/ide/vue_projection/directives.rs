#![allow(dead_code)]
//! Built-in and custom directive uses.

use rustc_hash::{FxHashMap, FxHashSet};
use verter_parser::types::{BindingType, BuiltinDirective as ParsedBuiltin, DirectiveKind};

use crate::ast::types::{AstNodeKind, TemplateAst};
use crate::framework_common::projection_plan::{
    classify_op, AdmittedExpressionId, AttributeOpKind, ElementNamespace, ElementUseKind,
    PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::branches::{TemplateBranchProjection, TemplateItem};
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, TemplateBlockId, UseHost,
};
use crate::ide::vue_projection::component_use::{quote, ComponentUseProjection};
use crate::ide::vue_projection::native_elements::NativeElementProjection;
use crate::ide::vue_projection::script_setup::{ScriptProjectionFacts, SourceRange};
use crate::template::vue_syntax::to_pascal_case;

/// The directive contracts are declared in the checking module itself, so a
/// project needs no Verter types package to check a directive.
pub const DIRECTIVE_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "42-directives",
    text: crate::ide::script::DIRECTIVE_CHECKING_HELPERS,
};

/// `v-pre` subtrees never reach the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinDirective {
    Show,
    Text,
    Html,
    Once,
    Memo,
    Cloak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveSource {
    Builtin(BuiltinDirective),
    /// A setup binding named `vName`.
    Local {
        binding: String,
    },
    /// `GlobalDirectives` lookup by name.
    Global {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveUseObligation {
    pub host: UseHost,
    pub block: TemplateBlockId,
    pub op_index: u32,
    pub source: DirectiveSource,
    pub value: Option<AdmittedExpressionId>,
    pub argument: Option<AdmittedExpressionId>,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveBindingContext {
    pub host: UseHost,
    pub op_index: u32,
    /// Type text of the directive's declared binding over the host element or instance.
    pub binding_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub uses: Vec<DirectiveUseObligation>,
    pub contexts: Vec<DirectiveBindingContext>,
    placed: Vec<PlacedStatement>,
}

impl DirectiveProjection {
    /// The product before directives are checked: it contributes nothing.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            uses: Vec::new(),
            contexts: Vec::new(),
            placed: Vec::new(),
        }
    }
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.contexts.is_empty() {
            Vec::new()
        } else {
            vec![DIRECTIVE_PRELUDE]
        }
    }
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        self.placed.clone()
    }

    /// A custom component check can only read a witness the composer emits.
    pub(super) fn retain_component_hosts<'a>(
        &mut self,
        hosts: impl IntoIterator<Item = &'a UseHost>,
    ) {
        let hosts: FxHashSet<_> = hosts.into_iter().collect();
        let count = self.placed.len();
        self.placed.retain(|statement| {
            statement.phase != StatementPhase::After
                || !matches!(statement.host, Some(UseHost::Component(_)))
                || statement
                    .host
                    .as_ref()
                    .is_some_and(|host| hosts.contains(host))
        });
        self.complete &= self.placed.len() == count;
        self.contexts.retain(|context| {
            !matches!(context.host, UseHost::Component(_)) || hosts.contains(&context.host)
        });
    }
}

/// Borrowed syntax facts from the same parse as the plan. Static arguments
/// are parser spans; only dynamic arguments are admitted plan expressions.
#[derive(Debug, Clone, Copy)]
pub struct DirectiveSyntax<'a> {
    pub source: &'a str,
    pub ast: &'a TemplateAst,
    pub binding_kinds: &'a [(String, BindingType)],
}

/// Check admitted directive uses in their existing template blocks. Names
/// and static argument/modifier geometry come from the parser, setup names
/// from its binding classifications, and assignability from the shared
/// custom-directive helper. No directive opens a lexical scope.
#[must_use]
pub fn project_directives(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    uses: &ComponentUseProjection,
    _native: &NativeElementProjection,
    _script: &ScriptProjectionFacts,
    syntax: DirectiveSyntax<'_>,
) -> DirectiveProjection {
    let mut projection = DirectiveProjection::pending(plan.snapshot.clone());
    projection.pending = false;
    projection.complete = plan.is_complete() && branches.complete;
    let bindings: FxHashMap<&str, BindingType> = syntax
        .binding_kinds
        .iter()
        .map(|(name, kind)| (name.as_str(), *kind))
        .collect();
    let witnesses: FxHashMap<_, _> = uses
        .witnesses
        .iter()
        .map(|witness| (&witness.use_id, witness))
        .collect();
    let hosts = plan
        .uses
        .iter()
        .map(|use_| {
            (
                UseHost::Component(use_.id.clone()),
                TemplateItem::Component(use_.id.clone()),
                &use_.operations,
                use_.syntax_node,
                witnesses
                    .get(&use_.id)
                    .map(|witness| witness.binding.clone()),
            )
        })
        .chain(plan.elements.iter().map(|element| {
            (
                UseHost::Element(element.id.clone()),
                TemplateItem::Element(element.id.clone()),
                &element.operations,
                element.syntax_node,
                native_host(&element.kind),
            )
        }));
    for (host, item, operations, syntax_node, host_value) in hosts {
        if !operations
            .iter()
            .any(|op| op.kind == AttributeOpKind::Directive)
        {
            continue;
        }
        let Some((block, item)) = branches.place_of(&item) else {
            projection.complete = false;
            continue;
        };
        let Some(AstNodeKind::Element(element)) = syntax
            .ast
            .nodes
            .get(syntax_node as usize)
            .map(|node| &node.kind)
        else {
            projection.complete = false;
            continue;
        };
        let mut props = element
            .props
            .iter()
            .filter(|prop| classify_op(prop, syntax.source).is_some());
        for op in operations {
            let Some(prop) = props.next() else {
                projection.complete = false;
                break;
            };
            if op.kind != AttributeOpKind::Directive {
                continue;
            }
            let source = match prop.kind(syntax.source.as_bytes()) {
                DirectiveKind::Builtin(kind) => DirectiveSource::Builtin(match kind {
                    ParsedBuiltin::Show => BuiltinDirective::Show,
                    ParsedBuiltin::Text => BuiltinDirective::Text,
                    ParsedBuiltin::Html => BuiltinDirective::Html,
                    ParsedBuiltin::Memo => BuiltinDirective::Memo,
                    // The parser omits pre subtrees and caches once separately.
                    ParsedBuiltin::Once | ParsedBuiltin::Cloak => continue,
                    _ => continue,
                }),
                DirectiveKind::Custom => {
                    let binding = format!("v{}", to_pascal_case(&op.name));
                    if bindings.get(binding.as_str()).is_some_and(|kind| {
                        kind.is_setup() || matches!(kind, BindingType::PropsDestructured)
                    }) {
                        DirectiveSource::Local { binding }
                    } else {
                        DirectiveSource::Global { name: binding }
                    }
                }
                _ => {
                    projection.complete = false;
                    continue;
                }
            };
            let mut fragment = CheckingFragment::new();
            let phase;
            match &source {
                DirectiveSource::Builtin(_) => {
                    phase = StatementPhase::Before;
                    if let Some(value) = &op.expression {
                        let carried_by_use = match &host {
                            UseHost::Component(id) => witnesses.get(id).is_some_and(|witness| {
                                witness
                                    .transaction
                                    .members
                                    .iter()
                                    .any(|member| member.op_index() == op.index)
                                    || witness
                                        .transaction
                                        .validations
                                        .iter()
                                        .any(|check| check.op_index == op.index)
                            }),
                            UseHost::Element(_) => false,
                        };
                        if !carried_by_use {
                            fragment
                                .push_synth("(")
                                .push_expression(value.clone())
                                .push_synth(");\n");
                        }
                    }
                }
                DirectiveSource::Local { .. } | DirectiveSource::Global { .. } => {
                    let Some(host_value) = &host_value else {
                        projection.complete = false;
                        continue;
                    };
                    phase = StatementPhase::After;
                    let check = format!("__VerterDirectiveCheck{}", projection.contexts.len());
                    let name_range = SourceRange {
                        start: prop.start,
                        end: prop.name_end,
                    };
                    fragment.push_synth(format!(
                        "const {check} = ___VERTER___runCustomDirective({host_value}, "
                    ));
                    let directive = match &source {
                        DirectiveSource::Local { binding } => {
                            // The template context uses this same Vue shallow read view.
                            format!("___VERTER___shallowUnwrapRef({{ {binding} }}).{binding}")
                        }
                        DirectiveSource::Global { name } => {
                            format!("___VERTER___retrieveSetupDirectives({{}})[{}]", quote(name))
                        }
                        DirectiveSource::Builtin(_) => unreachable!(),
                    };
                    fragment
                        .push_alias(directive, name_range)
                        .push_synth(format!(");\n{check}("))
                        .push_alias(host_value.clone(), name_range)
                        .push_synth(", (");
                    if let Some(value) = &op.expression {
                        fragment.push_expression(value.clone());
                    } else {
                        fragment.push_alias("undefined", name_range);
                    }
                    fragment.push_synth("), ");
                    if let Some(argument) = &op.argument {
                        fragment
                            .push_synth("(")
                            .push_expression(argument.clone())
                            .push_synth(")");
                    } else if let (Some(start), Some(end)) = (prop.arg_start, prop.arg_end) {
                        if prop.is_dynamic == Some(true) {
                            projection.complete = false;
                            continue;
                        }
                        let Some(argument) = syntax.source.get(start as usize..end as usize) else {
                            projection.complete = false;
                            continue;
                        };
                        fragment.push_alias(quote(argument), SourceRange { start, end });
                    } else {
                        fragment.push_synth("undefined");
                    }
                    fragment.push_synth(", {");
                    let mut seen_modifiers = FxHashSet::default();
                    for modifier in &prop.modifiers {
                        let Some(name) = syntax
                            .source
                            .get(modifier.start as usize..modifier.end as usize)
                        else {
                            projection.complete = false;
                            continue;
                        };
                        if !seen_modifiers.insert(name) {
                            continue;
                        }
                        if seen_modifiers.len() > 1 {
                            fragment.push_synth(", ");
                        }
                        fragment
                            .push_alias(
                                quote(name),
                                SourceRange {
                                    start: modifier.start,
                                    end: modifier.end,
                                },
                            )
                            .push_synth(": true");
                    }
                    fragment.push_synth("});\n");
                    projection.contexts.push(DirectiveBindingContext {
                        host: host.clone(),
                        op_index: op.index,
                        binding_type: format!("import(\"vue\").DirectiveBinding<Parameters<typeof {check}>[1], keyof Parameters<typeof {check}>[3] & string, Exclude<Parameters<typeof {check}>[2], undefined>>"),
                    });
                }
            }
            if !fragment.is_empty() {
                projection.placed.push(PlacedStatement {
                    anchor: StatementAnchor::Item { block, item },
                    phase,
                    family: ObligationFamily::Directive,
                    host: Some(host.clone()),
                    fragment,
                });
            }
            projection.uses.push(DirectiveUseObligation {
                host: host.clone(),
                block,
                op_index: op.index,
                source,
                value: op.expression.clone(),
                argument: op.argument.clone(),
                modifiers: op.modifiers.clone(),
            });
        }
        if props.next().is_some() {
            projection.complete = false;
        }
    }
    projection
}

/// Native hosts use the DOM library's namespace tables, preserving exact
/// tag types without a second Rust element-type table.
fn native_host(kind: &ElementUseKind) -> Option<String> {
    match kind {
        ElementUseKind::Native { tag, namespace } => {
            let (map, base) = match namespace {
                ElementNamespace::Html => ("HTMLElementTagNameMap", "HTMLElement"),
                ElementNamespace::Svg => ("SVGElementTagNameMap", "SVGElement"),
                ElementNamespace::MathMl => ("MathMLElementTagNameMap", "MathMLElement"),
            };
            // Vue's admitted tag inventory includes entries absent from lib.dom.
            Some(format!(
                "(null! as ({map} & Record<string, {base}>)[{}])",
                quote(tag)
            ))
        }
        ElementUseKind::CustomElement { .. } => Some("(null! as HTMLElement)".to_string()),
        // These syntax-only wrappers have no single DOM host.
        ElementUseKind::SlotOutlet | ElementUseKind::TemplateWrapper => None,
    }
}
