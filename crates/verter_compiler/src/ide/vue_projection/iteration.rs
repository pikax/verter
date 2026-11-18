//! `v-for` as a statement scope with Vue's iteration contract.

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, BinderKind, BindingOriginId, ExpressionKind, ExpressionOccurrence,
    LexicalScopeId, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::branches::{TemplateBlockKind, TemplateBranchProjection};
use crate::ide::vue_projection::checking_fragment::{
    BlockHead, CheckingFragment, PreludeDeclaration, TemplateBlockId,
};
use crate::ide::vue_projection::script_setup::SourceRange;
use crate::utils::oxc::vue::parse_vfor_with_bindings;

/// `__VerterIterate`: one overload per Vue iteration source kind, returning
/// an array of `[value, key, index]` tuples.
///
/// An array or tuple gives `[element, number, number]`, as does any other
/// iterable; a string gives its characters, a number `1..n`, and an object its
/// values with the key as `string`. A literal `null` or `undefined` source
/// renders nothing. A source that merely may be nullish matches no overload,
/// so reading it unguarded is an error at the source, and the same loop under
/// a guard that narrows it away is clean.
pub const ITERATION_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "20-iteration",
    text: concat!(
        "declare function __VerterIterate<T>(source: readonly T[]): [T, number, number][];\n",
        "declare function __VerterIterate(source: string): [string, number, number][];\n",
        "declare function __VerterIterate(source: number): [number, number, number][];\n",
        "declare function __VerterIterate<T>(source: Iterable<T>): [T, number, number][];\n",
        "declare function __VerterIterate(source: null | undefined): [never, never, never][];\n",
        "declare function __VerterIterate<T extends object>(source: T): [T[keyof T], string, number][];\n",
    ),
};

/// Position of an alias in `(value, key, index)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterationPosition {
    Value,
    Key,
    Index,
}

impl IterationPosition {
    const ALL: [Self; 3] = [Self::Value, Self::Key, Self::Index];
}

/// The typed iteration of one loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueIterationContract {
    pub block: TemplateBlockId,
    pub source: AdmittedExpressionId,
    /// Authored alias pattern per position, verbatim; `None` when omitted.
    pub value: Option<AdmittedExpressionId>,
    pub key: Option<AdmittedExpressionId>,
    pub index: Option<AdmittedExpressionId>,
    /// Binding the checked source is held in, so the source is evaluated
    /// before the aliases come into scope.
    pub source_binding: String,
}

/// The lexical scope a loop opens. Outer bindings come back on block exit
/// because the scope is a real block; there is no substitution table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateLoopScope {
    pub block: TemplateBlockId,
    pub lexical_env: LexicalScopeId,
    pub binders: Vec<BindingOriginId>,
}

/// One name bound by an alias pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestructuredBinding {
    pub name: String,
    pub origin: BindingOriginId,
    pub position: IterationPosition,
    pub range: SourceRange,
}

/// Names bound by the alias patterns of one loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestructuredBindingOrigins {
    pub block: TemplateBlockId,
    pub bindings: Vec<DestructuredBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IterationProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub contracts: Vec<VueIterationContract>,
    pub scopes: Vec<TemplateLoopScope>,
    pub origins: Vec<DestructuredBindingOrigins>,
}

impl IterationProjection {
    /// The product before loops are projected: it contributes nothing.
    #[must_use]
    #[cfg(test)]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            contracts: Vec::new(),
            scopes: Vec::new(),
            origins: Vec::new(),
        }
    }

    #[must_use]
    pub fn contract(&self, block: TemplateBlockId) -> Option<&VueIterationContract> {
        self.contracts
            .iter()
            .find(|contract| contract.block == block)
    }

    /// `const <source_binding> = __VerterIterate(<source>);` then
    /// `for (const [<value>, <key>, <index>] of <source_binding>) {`.
    #[must_use]
    pub fn loop_head(&self, block: TemplateBlockId) -> Option<BlockHead> {
        let contract = self.contract(block)?;
        let mut before = CheckingFragment::new();
        before
            .push_synth(format!(
                "const {} = __VerterIterate((",
                contract.source_binding
            ))
            .push_expression(contract.source.clone())
            .push_synth("));\n");
        let mut open = CheckingFragment::synth("for (const [");
        let aliases = [&contract.value, &contract.key, &contract.index];
        for (ordinal, alias) in aliases.into_iter().flatten().enumerate() {
            if ordinal > 0 {
                open.push_synth(", ");
            }
            open.push_expression(alias.clone());
        }
        open.push_synth(format!("] of {}) {{\n", contract.source_binding));
        Some(BlockHead { before, open })
    }

    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.contracts.is_empty() {
            Vec::new()
        } else {
            vec![ITERATION_PRELUDE]
        }
    }
}

/// The loop of every `Loop` block: its source, its alias patterns and the
/// names they bind. A loop whose aliases the plan did not admit is left out
/// and the projection is incomplete.
#[must_use]
pub fn project_iteration(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
) -> IterationProjection {
    let mut projection = IterationProjection {
        snapshot: plan.snapshot.clone(),
        complete: branches.complete,
        pending: false,
        contracts: Vec::new(),
        scopes: Vec::new(),
        origins: Vec::new(),
    };
    for block in &branches.blocks {
        let TemplateBlockKind::Loop { source } = &block.kind else {
            continue;
        };
        let Some(occurrence) = plan.expression(source) else {
            projection.complete = false;
            continue;
        };
        let aliases = alias_occurrences(plan, occurrence);
        if aliases.is_empty() || aliases.len() > IterationPosition::ALL.len() {
            projection.complete = false;
            continue;
        }
        let Some(bindings) = destructured_bindings(plan, &block.lexical_env, &aliases) else {
            projection.complete = false;
            continue;
        };
        let alias_id = |position: usize| aliases.get(position).map(|alias| alias.id.clone());
        projection.contracts.push(VueIterationContract {
            block: block.id,
            source: source.clone(),
            value: alias_id(0),
            key: alias_id(1),
            index: alias_id(2),
            source_binding: format!("__VerterEach{}", block.id.0),
        });
        projection.scopes.push(TemplateLoopScope {
            block: block.id,
            lexical_env: block.lexical_env.clone(),
            binders: bindings
                .iter()
                .map(|binding| binding.origin.clone())
                .collect(),
        });
        projection.origins.push(DestructuredBindingOrigins {
            block: block.id,
            bindings,
        });
    }
    projection
}

/// The alias patterns of the loop whose source is `source`, by position. The
/// plan admits them on the loop's own node, in the scope the loop opens.
fn alias_occurrences<'p>(
    plan: &'p ProjectionPlan,
    source: &ExpressionOccurrence,
) -> Vec<&'p ExpressionOccurrence> {
    let mut aliases: Vec<&ExpressionOccurrence> = plan
        .expressions()
        .iter()
        .filter(|occurrence| {
            occurrence.kind == ExpressionKind::VForAlias
                && occurrence.syntax_node == source.syntax_node
        })
        .collect();
    aliases.sort_by_key(|occurrence| occurrence.start);
    aliases
}

/// Each name the alias patterns declare, with the origin the plan minted for
/// it and its range in the authored pattern. `None` when a pattern does not
/// parse or a name has no origin.
fn destructured_bindings(
    plan: &ProjectionPlan,
    env: &LexicalScopeId,
    aliases: &[&ExpressionOccurrence],
) -> Option<Vec<DestructuredBinding>> {
    let mut bindings = Vec::new();
    for (alias, position) in aliases.iter().zip(IterationPosition::ALL) {
        // The v-for parse owns which names a pattern declares: the alias is
        // read as the left side of a loop, so its locals are offsets into it.
        let allocator = Allocator::default();
        let text = format!("{} in _", alias.spelling);
        let parsed = parse_vfor_with_bindings(&allocator, &text, SourceType::tsx(), &[]);
        if !parsed.is_ok() {
            return None;
        }
        for local in &parsed.locals {
            let name = text.get(local.start as usize..local.end as usize)?;
            let origin = plan.origins.iter().find(|origin| {
                origin.kind == BinderKind::VForAlias
                    && origin.lexical_env == *env
                    && origin.name == name
            })?;
            bindings.push(DestructuredBinding {
                name: name.to_string(),
                origin: origin.id.clone(),
                position,
                range: SourceRange {
                    start: alias.start + local.start,
                    end: alias.start + local.end,
                },
            });
        }
    }
    Some(bindings)
}
