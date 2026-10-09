//! Comp-function emission for IDE TSX template projection (D9 of
//! ownership-domain analysis).
//!
//! This is the largest sibling. Hosts:
//! - `emit_comp_functions_to_string` / `emit_get_root_component_to_string`:
//!   the two public entry points.
//! - `CompScope`: the scope-stack enum used while walking the template
//!   arena.
//! - `ComponentFlow`: the conditional structure component functions read
//!   their narrowing from, emitted once per chain.
//! - `extract_vslot_binding_names` / `extract_vfor_binding_names`,
//!   `find_scope_for_tag`, `walk_children_for_comp`,
//!   `serialize_element_props`, `resolve_all_prop_refs_in_expr`, the
//!   `collect_prop_refs*` family, `collect_binding_pattern_names`, and
//!   `emit_comp_function_for_element`.

use crate::ast::types::{
    AstNodeKind, ConditionalChain, ElementNode, ElementNodeConditionKind, TagType, TemplateAst,
};
use crate::ide::event_to_jsx_name;

use super::{to_pascal_case, PREFIX};

/// Emit Comp{offset} functions to a string buffer (inside templateBindingFN).
/// Emit Comp functions for all template elements and collect root element info.
///
/// Returns `(root_comp_entries, all_comp_offsets)` where each root comp entry
/// is `(offset, props_literal, condition_text)` for elements that are direct
/// children of the template root. `condition_text` is `Some(expr)` for
/// v-if/v-else-if branches and `None` for v-else.
/// A single v-if/v-else-if/v-else chain produces multiple entries (one per
/// branch) that get unioned in `getRootComponent`.
/// A true fragment (multiple independent root elements) returns empty root entries.
#[allow(clippy::type_complexity)]
pub(super) fn emit_comp_functions_to_string(
    buf: &mut String,
    gs: &str,
    gn: &str,
    template_ast: Option<&TemplateAst>,
    source: &str,
    is_jsx: bool,
    prop_names: &rustc_hash::FxHashSet<&str>,
) -> (Vec<(u32, String, Option<String>)>, Vec<u32>) {
    let ast = match template_ast {
        Some(a) => a,
        None => return (vec![], vec![]),
    };

    let root_children = ast
        .root
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);

    // Count root elements excluding v-else / v-else-if (they don't create
    // additional roots — they're part of the same conditional chain).
    // If there are multiple independent root elements it's a fragment and
    // attrs fallthrough does not apply (Vue warns at runtime).
    let root_element_count = root_children
        .iter()
        .filter(|id| {
            if let AstNodeKind::Element(el) = &ast.nodes[id.0].kind {
                !matches!(
                    el.v_condition.as_ref().map(|c| &c.kind),
                    Some(
                        crate::ast::types::ElementNodeConditionKind::Else
                            | crate::ast::types::ElementNodeConditionKind::ElseIf
                    )
                )
            } else {
                false
            }
        })
        .count();

    // A root-level v-for expands to multiple rendered roots, so attrs
    // fallthrough behaves like a fragment rather than a single root element.
    let root_has_v_for = root_children.iter().any(|id| {
        matches!(
            &ast.nodes[id.0].kind,
            AstNodeKind::Element(el)
                if el.v_for.is_some()
                    && !matches!(
                        el.v_condition.as_ref().map(|c| &c.kind),
                        Some(
                            crate::ast::types::ElementNodeConditionKind::Else
                                | crate::ast::types::ElementNodeConditionKind::ElseIf
                        )
                    )
        )
    });

    // Only emit Comp functions for root elements when it's a single root
    // (possibly a conditional chain). Fragments don't support attrs fallthrough.
    let emit_root_comps = root_element_count <= 1 && !root_has_v_for;

    let mut root_comp_entries: Vec<(u32, String, Option<String>)> = Vec::new();
    let mut all_comp_offsets: Vec<u32> = Vec::new();
    let mut flow = ComponentFlow::default();

    walk_children_for_comp(
        buf,
        gs,
        gn,
        ast,
        source,
        root_children,
        ast.root
            .content
            .as_ref()
            .map(|c| c.v_if_chains.as_slice())
            .unwrap_or(&[]),
        None,
        &mut flow,
        &[],
        &mut root_comp_entries,
        &mut all_comp_offsets,
        emit_root_comps,
        is_jsx,
        prop_names,
    );
    flow.emit(buf, gs, gn);

    (root_comp_entries, all_comp_offsets)
}

/// Emit getRootComponent + getRootComponentPassedProps to a string buffer.
///
/// When `root_comp_entries` has multiple entries (v-if/v-else chain), the
/// return type is a union of all branch Comp functions so that `RootElement`
/// correctly resolves to the union of all possible root element types.
/// `getRootComponentPassedProps` unions all branch props so that Omit removes
/// props used by ANY branch.
///
/// When `narrowing` is `Some(result)`, conditional types are used instead of
/// `Math.random()` union, and narrowing generics are appended to the function.
pub(super) fn emit_get_root_component_to_string(
    buf: &mut String,
    gs: &str,
    gn: &str,
    root_comp_entries: &[(u32, String, Option<String>)],
    narrowing: Option<&crate::ide::condition_narrowing::ConditionalRootNarrowing>,
) {
    use std::fmt::Write;

    if root_comp_entries.is_empty() {
        write!(
            buf,
            "\nfunction {P}getRootComponent{gs}() {{ return {{}}; }}\
             \nfunction {P}getRootComponentPassedProps{gs}() {{ return {{}}; }}",
            P = PREFIX,
            gs = gs,
        )
        .expect("write to String is infallible");
        return;
    }

    // ── Build narrowing generics string ────────────────────────
    // When narrowing is active, append T_{prop} generics to the function signature
    // and use conditional types instead of Math.random() union.
    let narrowing_gs = if let Some(nr) = narrowing {
        let mut extra = String::new();
        for g in &nr.generics {
            if !extra.is_empty() {
                extra.push_str(", ");
            }
            write!(
                extra,
                "T_{prop} extends {P}defineProps_Type['{prop}'] = {P}defineProps_Type['{prop}']",
                prop = g.prop_name,
                P = PREFIX,
            )
            .expect("write to String is infallible");
        }
        // Merge with existing gs: gs is like "<T extends string>" or ""
        if gs.is_empty() {
            format!("<{extra}>")
        } else {
            // gs starts with < and ends with >, insert before closing >
            format!("{}, {extra}>", &gs[..gs.len() - 1])
        }
    } else {
        gs.to_string()
    };

    // getRootComponent
    write!(
        buf,
        "\nfunction {P}getRootComponent{ngs}() {{ ",
        P = PREFIX,
        ngs = narrowing_gs
    )
    .expect("write to String is infallible");

    if root_comp_entries.len() == 1 {
        let (offset, _, _) = &root_comp_entries[0];
        write!(
            buf,
            "return {P}Comp{offset}{gn}();",
            P = PREFIX,
            offset = offset,
            gn = gn,
        )
        .expect("write to String is infallible");
    } else if let Some(nr) = narrowing {
        // Narrowing: emit `return {} as T_foo extends true ? ReturnType<typeof Comp1<...>> : ...`
        buf.push_str("return {} as ");
        for (i, branch) in nr.branches.iter().enumerate() {
            if let Some(ref cond) = branch.narrowing {
                let extends_rhs = if let Some(ref lit) = cond.literal {
                    lit.clone()
                } else if cond.negated {
                    "false".to_string()
                } else {
                    "true".to_string()
                };
                write!(
                    buf,
                    "T_{prop} extends {rhs} ? ReturnType<typeof {P}Comp{offset}{gn}> : ",
                    prop = cond.prop_name,
                    rhs = extends_rhs,
                    P = PREFIX,
                    offset = branch.comp_offset,
                    gn = gn,
                )
                .expect("write to String is infallible");
            } else {
                // v-else: terminal fallback
                write!(
                    buf,
                    "ReturnType<typeof {P}Comp{offset}{gn}>",
                    P = PREFIX,
                    offset = branch.comp_offset,
                    gn = gn,
                )
                .expect("write to String is infallible");
            }
            // If last branch has a condition (no v-else), add never fallback
            if i == nr.branches.len() - 1 && branch.narrowing.is_some() {
                buf.push_str("never");
            }
        }
        buf.push(';');
    } else {
        // Fallback: Math.random() union
        for (i, (offset, _, _)) in root_comp_entries.iter().enumerate() {
            if i == root_comp_entries.len() - 1 {
                write!(
                    buf,
                    "return {P}Comp{offset}{gn}();",
                    P = PREFIX,
                    offset = offset,
                    gn = gn,
                )
                .expect("write to String is infallible");
            } else {
                write!(
                    buf,
                    "if (Math.random()) return {P}Comp{offset}{gn}(); else ",
                    P = PREFIX,
                    offset = offset,
                    gn = gn,
                )
                .expect("write to String is infallible");
            }
        }
    }
    write!(buf, " }}").expect("write to String is infallible");

    // getRootComponentPassedProps: union of all branch props
    write!(
        buf,
        "\nfunction {P}getRootComponentPassedProps{ngs}() {{ ",
        P = PREFIX,
        ngs = narrowing_gs,
    )
    .expect("write to String is infallible");
    if root_comp_entries.len() == 1 {
        let (_, props, _) = &root_comp_entries[0];
        write!(buf, "return {props};", props = props).expect("write to String is infallible");
    } else {
        // Union: same pattern so Omit removes props used by any branch
        for (i, (_, props, _)) in root_comp_entries.iter().enumerate() {
            if i == root_comp_entries.len() - 1 {
                write!(buf, "return {props};", props = props)
                    .expect("write to String is infallible");
            } else {
                write!(
                    buf,
                    "if (Math.random()) return {props}; else ",
                    props = props
                )
                .expect("write to String is infallible");
            }
        }
    }
    write!(buf, " }}").expect("write to String is infallible");
}

/// Scope introduced by v-slot or v-for that provides template-local bindings.
///
/// When a Comp function's tag name comes from one of these scopes (e.g.
/// `<Comp ref="x" />` inside `<MyComp v-slot="{ Comp }">`), the Comp function
/// must reconstruct the type through the parent's instantiated slot type rather
/// than referencing the tag name directly (which isn't in scope at the top level).
#[derive(Debug, Clone)]
enum CompScope {
    /// v-slot on a component: bindings come from the parent's slot props.
    VSlot {
        /// Offset of the parent element's Comp function (its tag_open.start).
        parent_comp_offset: u32,
        /// Slot name ("default" for `v-slot`, custom for `#name`).
        slot_name: String,
        /// Raw scope expression text (e.g. "{ Comp, data }" or "data").
        params_expr: String,
        /// Binding names extracted from the destructuring (e.g. ["Comp", "data"]).
        binding_names: Vec<String>,
    },
    /// v-for: bindings come from iterating over an expression.
    VFor {
        /// The iterable source expression (e.g. "components").
        iterable_expr: String,
        /// The iterator variable names (e.g. ["comp"] or ["comp", "index"]).
        binding_names: Vec<String>,
    },
}

/// Extract binding names from a v-slot scope expression.
///
/// Handles destructuring: `"{ Comp, data }"` → `["Comp", "data"]`
/// and simple: `"data"` → `["data"]`.
fn extract_vslot_binding_names(expr: &str) -> Vec<String> {
    let trimmed = expr.trim();
    if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        // Destructuring: { Comp, data, other: alias }
        inner
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    return None;
                }
                // Handle renaming: `original: alias` → use `alias`
                // Handle rest: `...rest` → use `rest`
                if let Some(alias) = part.split(':').nth(1) {
                    Some(alias.trim().to_string())
                } else if let Some(rest) = part.strip_prefix("...") {
                    Some(rest.trim().to_string())
                } else {
                    // Handle default value: `name = default` → use `name`
                    let name = part.split('=').next().unwrap_or(part).trim();
                    Some(name.to_string())
                }
            })
            .collect()
    } else {
        // Simple binding: data
        vec![trimmed.to_string()]
    }
}

/// Extract v-for iterator binding names from the params portion (before "in"/"of").
///
/// `"comp"` → `["comp"]`
/// `"comp, index"` → `["comp", "index"]`
fn extract_vfor_binding_names(params: &str) -> Vec<String> {
    params
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                None
            } else {
                Some(p.to_string())
            }
        })
        .collect()
}

/// Check if a tag name is introduced by any scope in the chain.
/// Returns the innermost scope that introduces this binding.
fn find_scope_for_tag<'a>(tag_name: &str, comp_scopes: &'a [CompScope]) -> Option<&'a CompScope> {
    // Walk from innermost (last) to outermost (first)
    for scope in comp_scopes.iter().rev() {
        let names = match scope {
            CompScope::VSlot { binding_names, .. } => binding_names,
            CompScope::VFor { binding_names, .. } => binding_names,
        };
        if names.iter().any(|n| n == tag_name) {
            return Some(scope);
        }
    }
    None
}

/// A branch of the component-function flow: one `v-if` / `v-else-if` /
/// `v-else` member, addressed by its chain and position.
#[derive(Debug, Clone, Copy)]
struct FlowRegionRef {
    chain: usize,
    branch: usize,
}

/// The conditional structure component functions read their narrowing from.
///
/// TypeScript narrows a reference only along one function's flow, so a
/// component function that re-stated its condition path would grow with the
/// path (every enclosing positive and predecessor negation). Instead each
/// chain enclosing a component function is emitted ONCE — the outermost as a
/// function, a nested one as an immediately invoked block, which continues its
/// enclosing flow — and every branch evaluates its components' tags and props
/// in the narrowed flow, returning each behind a deferred instantiation:
///
/// ```text
/// function ___VERTER___Flow12() {
///   if (A) {
///   const ___VERTER___s20 = { t: Foo, p: {"x": state.a} };
///   const ___VERTER___f40 = (() => { … })();
///   return { b: 0, c20: () => ___VERTER___instantiateComponent(___VERTER___s20.t, ___VERTER___s20.p), f40: ___VERTER___f40 } as const;
///   } else if (B) {
///   return { b: 1 } as const;
///   }
///   return null;
/// }
/// function ___VERTER___R12() { const f = ___VERTER___Flow12(); return f !== null && f.b === 0 ? f : null; }
/// function ___VERTER___Comp20() { const r = ___VERTER___R12(); return r === null ? null : r.c20(); }
/// ```
///
/// Every authored condition appears once, each live branch has one
/// constant-size navigator reading its parent's, and each component function
/// reads its branch's navigator, so the output is linear in the template. A
/// component function still returns `<instance> | null`. The deferred call
/// keeps a branch record's type independent of what its components
/// instantiate: a component whose props read another one's template ref does
/// not make the chain's inferred type circular.
#[derive(Default)]
struct ComponentFlow {
    chains: Vec<FlowChain>,
}

struct FlowChain {
    parent: Option<FlowRegionRef>,
    branches: Vec<FlowBranch>,
    /// Some branch holds a component function, directly or nested.
    live: bool,
}

struct FlowBranch {
    /// The member's `tag_open.start`, naming its navigator.
    offset: u32,
    /// The resolved condition; `None` for `v-else`.
    test: Option<String>,
    items: Vec<FlowItem>,
    live: bool,
}

enum FlowItem {
    /// A component function's tag and props, captured in the branch's flow.
    Component { offset: u32, capture: String },
    /// A chain nested in the branch.
    Chain(usize),
}

impl ComponentFlow {
    /// Open a chain inside `parent` (or at the top level).
    fn open_chain(&mut self, parent: Option<FlowRegionRef>) -> usize {
        let chain = self.chains.len();
        self.chains.push(FlowChain {
            parent,
            branches: Vec::new(),
            live: false,
        });
        if let Some(parent) = parent {
            self.chains[parent.chain].branches[parent.branch]
                .items
                .push(FlowItem::Chain(chain));
        }
        chain
    }

    /// Append the member at `offset` to `chain`.
    fn open_branch(&mut self, chain: usize, offset: u32, test: Option<String>) -> FlowRegionRef {
        crate::ide::template::flow::record(|work| work.component_branches += 1);
        let branches = &mut self.chains[chain].branches;
        branches.push(FlowBranch {
            offset,
            test,
            items: Vec::new(),
            live: false,
        });
        FlowRegionRef {
            chain,
            branch: branches.len() - 1,
        }
    }

    /// Capture a component function's tag and props in `region`'s flow.
    /// `bindings` are statements binding names the tag and props read.
    fn add_component(
        &mut self,
        region: FlowRegionRef,
        offset: u32,
        bindings: &str,
        tag: &str,
        props: &str,
    ) {
        let capture = if bindings.is_empty() {
            format!("{{ t: {tag}, p: {props} }}")
        } else {
            format!("(() => {{{bindings}\n  return {{ t: {tag}, p: {props} }};\n  }})()")
        };
        self.chains[region.chain].branches[region.branch]
            .items
            .push(FlowItem::Component { offset, capture });
        // Mark the branch and its ancestors live, stopping at one already live.
        let mut current = Some(region);
        while let Some(region) = current {
            let chain = &mut self.chains[region.chain];
            let branch = &mut chain.branches[region.branch];
            if branch.live {
                break;
            }
            branch.live = true;
            chain.live = true;
            current = chain.parent;
        }
    }

    /// The name suffix of `chain`: its first member's offset.
    fn chain_offset(&self, chain: usize) -> u32 {
        self.chains[chain].branches[0].offset
    }

    /// Emit every live chain once, its branches' navigators and the
    /// component functions reading them.
    fn emit(&self, buf: &mut String, gs: &str, gn: &str) {
        use std::fmt::Write;
        for (chain, flow_chain) in self.chains.iter().enumerate() {
            if !flow_chain.live {
                continue;
            }
            let chain_offset = self.chain_offset(chain);
            if flow_chain.parent.is_none() {
                crate::ide::template::flow::record(|work| work.component_functions += 1);
                write!(buf, "\nfunction {PREFIX}Flow{chain_offset}{gs}() {{")
                    .expect("write to String is infallible");
                self.write_chain(buf, chain);
                buf.push_str("\n}");
            }
            for (index, branch) in flow_chain.branches.iter().enumerate() {
                if !branch.live {
                    continue;
                }
                crate::ide::template::flow::record(|work| work.component_functions += 1);
                let offset = branch.offset;
                match flow_chain.parent {
                    None => write!(
                        buf,
                        "\nfunction {PREFIX}R{offset}{gs}() {{ const f = {PREFIX}Flow{chain_offset}{gn}(); \
                         return f !== null && f.b === {index} ? f : null; }}"
                    ),
                    Some(parent) => {
                        let parent_offset = self.chains[parent.chain].branches[parent.branch].offset;
                        write!(
                            buf,
                            "\nfunction {PREFIX}R{offset}{gs}() {{ const p = {PREFIX}R{parent_offset}{gn}(); \
                             const f = p === null ? null : p.f{chain_offset}; \
                             return f !== null && f.b === {index} ? f : null; }}"
                        )
                    }
                }
                .expect("write to String is infallible");
                for item in &branch.items {
                    if let FlowItem::Component { offset: comp, .. } = item {
                        crate::ide::template::flow::record(|work| work.component_functions += 1);
                        write!(
                            buf,
                            "\nfunction {PREFIX}Comp{comp}{gs}() {{ const r = {PREFIX}R{offset}{gn}(); \
                             return r === null ? null : r.c{comp}(); }}"
                        )
                        .expect("write to String is infallible");
                    }
                }
            }
        }
    }

    /// Write `chain`'s `if` / `else if` / `else` statement: each branch
    /// captures its components and nested live chains in its narrowed flow
    /// and returns its record.
    fn write_chain(&self, buf: &mut String, chain: usize) {
        use std::fmt::Write;
        let mut exhaustive = false;
        for (index, branch) in self.chains[chain].branches.iter().enumerate() {
            match (&branch.test, index) {
                (Some(test), 0) => write!(buf, "\n  if ({test}) {{"),
                (Some(test), _) => write!(buf, " else if ({test}) {{"),
                (None, 0) => write!(buf, "\n  {{"),
                (None, _) => write!(buf, " else {{"),
            }
            .expect("write to String is infallible");
            exhaustive |= branch.test.is_none();
            let mut fields = format!("b: {index}");
            for item in &branch.items {
                match item {
                    FlowItem::Component { offset, capture } => {
                        write!(buf, "\n  const {PREFIX}s{offset} = {capture};")
                            .expect("write to String is infallible");
                        write!(
                            fields,
                            ", c{offset}: () => {PREFIX}instantiateComponent({PREFIX}s{offset}.t, {PREFIX}s{offset}.p)"
                        )
                        .expect("write to String is infallible");
                    }
                    FlowItem::Chain(nested) if self.chains[*nested].live => {
                        crate::ide::template::flow::record(|work| work.component_functions += 1);
                        let nested_offset = self.chain_offset(*nested);
                        write!(buf, "\n  const {PREFIX}f{nested_offset} = (() => {{")
                            .expect("write to String is infallible");
                        self.write_chain(buf, *nested);
                        buf.push_str("\n  })();");
                        write!(fields, ", f{nested_offset}: {PREFIX}f{nested_offset}")
                            .expect("write to String is infallible");
                    }
                    FlowItem::Chain(_) => {}
                }
            }
            write!(buf, "\n  return {{ {fields} }} as const;\n  }}")
                .expect("write to String is infallible");
        }
        if !exhaustive {
            buf.push_str("\n  return null;");
        }
    }
}

/// Emit Comp{offset} functions for template elements.
/// Recursively walk children to emit Comp functions, recording the conditional
/// structure component functions read their narrowing from.
///
/// `region` is the innermost `v-if` / `v-else-if` / `v-else` branch enclosing
/// `children`; `chains` are the parser's chains among `children`.
#[allow(clippy::too_many_arguments)]
fn walk_children_for_comp(
    buf: &mut String,
    gs: &str,
    gn: &str,
    ast: &TemplateAst,
    source: &str,
    children: &[crate::types::NodeId],
    chains: &[ConditionalChain],
    region: Option<FlowRegionRef>,
    flow: &mut ComponentFlow,
    comp_scopes: &[CompScope],
    root_comp_entries: &mut Vec<(u32, String, Option<String>)>,
    all_comp_offsets: &mut Vec<u32>,
    emit_root_comps: bool,
    is_jsx: bool,
    prop_names: &rustc_hash::FxHashSet<&str>,
) {
    // The flow chain each authored chain of this list opened, by chain index.
    let mut opened: Vec<(usize, usize)> = Vec::new();
    for (index, &child_id) in children.iter().enumerate() {
        let node = &ast.nodes[child_id.0];
        if let AstNodeKind::Element(el) = &node.kind {
            // The element's own branch encloses it and its children.
            let region = match &el.v_condition {
                Some(condition) => {
                    let authored = chains
                        .iter()
                        .position(|chain| chain.member_indices.contains(&index));
                    let continued = authored.and_then(|chain| {
                        opened
                            .iter()
                            .find(|(opened_chain, _)| *opened_chain == chain)
                            .map(|&(_, flow_chain)| flow_chain)
                    });
                    let flow_chain = match continued {
                        Some(flow_chain) => flow_chain,
                        None => {
                            let flow_chain = flow.open_chain(region);
                            if let Some(chain) = authored {
                                opened.push((chain, flow_chain));
                            }
                            flow_chain
                        }
                    };
                    let test = match condition.kind {
                        ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                            Some(
                                match (condition.prop.value_start, condition.prop.value_end) {
                                    (Some(vs), Some(ve)) => resolve_all_prop_refs_in_expr(
                                        &source[vs as usize..ve as usize],
                                        prop_names,
                                    ),
                                    // A valueless condition narrows nothing.
                                    _ => format!("{PREFIX}flowBranch"),
                                },
                            )
                        }
                        ElementNodeConditionKind::Else => None,
                    };
                    Some(flow.open_branch(flow_chain, el.tag_open.start, test))
                }
                None => region,
            };

            // Build comp scope chain for v-slot and v-for
            let mut new_comp_scopes = comp_scopes.to_vec();

            // If this element is a component with v-slot, it needs a Comp function
            // (even without ref) so that child scope-aware Comp functions can reference it.
            let has_vslot_children =
                el.v_slot.is_some() && matches!(el.tag_type, TagType::Component);

            // If this element has v-slot, push a VSlot scope for its children
            if let Some(v_slot) = &el.v_slot {
                if matches!(el.tag_type, TagType::Component) {
                    let slot_name =
                        if let (Some(as_), Some(ae)) = (v_slot.arg_start, v_slot.arg_end) {
                            source[as_ as usize..ae as usize].to_string()
                        } else {
                            "default".to_string()
                        };
                    let params_expr =
                        if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                            source[vs as usize..ve as usize].to_string()
                        } else {
                            String::new()
                        };
                    let binding_names = if !params_expr.is_empty() {
                        extract_vslot_binding_names(&params_expr)
                    } else {
                        vec![]
                    };
                    new_comp_scopes.push(CompScope::VSlot {
                        parent_comp_offset: el.tag_open.start,
                        slot_name,
                        params_expr,
                        binding_names,
                    });
                }
            }

            // Check children for v-slot via <template #name="params"> (named slots)
            // These are handled when recursing into <template> elements below.

            // If this element has v-for, push a VFor scope
            if let Some(v_for) = &el.v_for {
                if let (Some(vs), Some(ve)) = (v_for.value_start, v_for.value_end) {
                    let full_expr = &source[vs as usize..ve as usize];
                    // Parse "item in items" → params="item", source_expr="items"
                    if let Some(sep_pos) = full_expr
                        .find(" in ")
                        .map(|p| (p, 4))
                        .or_else(|| full_expr.find(" of ").map(|p| (p, 4)))
                    {
                        let params = full_expr[..sep_pos.0].trim();
                        let iterable = full_expr[sep_pos.0 + sep_pos.1..].trim();
                        // Strip parens from params
                        let params = params
                            .strip_prefix('(')
                            .and_then(|p| p.strip_suffix(')'))
                            .unwrap_or(params);
                        let binding_names = extract_vfor_binding_names(params);
                        new_comp_scopes.push(CompScope::VFor {
                            iterable_expr: iterable.to_string(),
                            binding_names,
                        });
                    }
                }
            }

            // Emit Comp function for:
            // - elements with ref (always, for template ref typing)
            // - root elements when emit_root_comps=true (single root / conditional chain)
            // - component elements with v-slot (so child scope-aware Comp functions can reference parent)
            let is_eligible = !matches!(el.tag_type, TagType::SlotOutlet | TagType::Template);
            let needs_comp =
                is_eligible && (el.v_ref.is_some() || emit_root_comps || has_vslot_children);
            if needs_comp {
                let offset = el.tag_open.start;
                let props_lit = serialize_element_props(el, source, prop_names);
                emit_comp_function_for_element(
                    buf,
                    gs,
                    gn,
                    el,
                    source,
                    offset,
                    region,
                    flow,
                    &new_comp_scopes,
                    is_jsx,
                    &props_lit,
                );
                all_comp_offsets.push(offset);
                if emit_root_comps {
                    // Extract condition expression for narrowing analysis
                    let condition_text = el.v_condition.as_ref().and_then(|cond| {
                        use crate::ast::types::ElementNodeConditionKind;
                        match cond.kind {
                            ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                                let (Some(vs), Some(ve)) =
                                    (cond.prop.value_start, cond.prop.value_end)
                                else {
                                    return None;
                                };
                                Some(source[vs as usize..ve as usize].to_string())
                            }
                            ElementNodeConditionKind::Else => None,
                        }
                    });
                    root_comp_entries.push((offset, props_lit, condition_text));
                }
            }

            // Recurse into children
            if let Some(content) = &el.content {
                // For <template> elements with v-slot (named slots), push a VSlot scope
                // scoped to the template's children
                let child_comp_scopes = if matches!(el.tag_type, TagType::Template) {
                    if let Some(v_slot) = &el.v_slot {
                        let slot_name =
                            if let (Some(as_), Some(ae)) = (v_slot.arg_start, v_slot.arg_end) {
                                source[as_ as usize..ae as usize].to_string()
                            } else {
                                "default".to_string()
                            };
                        let params_expr =
                            if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                                source[vs as usize..ve as usize].to_string()
                            } else {
                                String::new()
                            };
                        let binding_names = if !params_expr.is_empty() {
                            extract_vslot_binding_names(&params_expr)
                        } else {
                            vec![]
                        };
                        // Find the parent component's comp offset by walking up
                        if let Some(parent_id) = node.parent {
                            let parent_node = &ast.nodes[parent_id.0];
                            if let AstNodeKind::Element(parent_el) = &parent_node.kind {
                                if matches!(parent_el.tag_type, TagType::Component) {
                                    let mut scopes_with_named = new_comp_scopes.clone();
                                    scopes_with_named.push(CompScope::VSlot {
                                        parent_comp_offset: parent_el.tag_open.start,
                                        slot_name,
                                        params_expr,
                                        binding_names,
                                    });
                                    scopes_with_named
                                } else {
                                    new_comp_scopes.clone()
                                }
                            } else {
                                new_comp_scopes.clone()
                            }
                        } else {
                            new_comp_scopes.clone()
                        }
                    } else {
                        new_comp_scopes.clone()
                    }
                } else {
                    new_comp_scopes.clone()
                };

                walk_children_for_comp(
                    buf,
                    gs,
                    gn,
                    ast,
                    source,
                    &content.children,
                    &content.v_if_chains,
                    region,
                    flow,
                    &child_comp_scopes,
                    root_comp_entries,
                    all_comp_offsets,
                    false,
                    is_jsx,
                    prop_names,
                );
            }
        }
    }
}

/// Serialize an element's template props as a TS object literal string.
///
/// Iterates `el.props` to produce `{"id": "app", "onClick": handler}`.
/// Skips `class` and `style` (Vue handles these specially).
/// Structural directives (v-if, v-for, etc.) are already taken out of `el.props`.
fn serialize_element_props(
    el: &ElementNode,
    source: &str,
    prop_names: &rustc_hash::FxHashSet<&str>,
) -> String {
    let mut entries: Vec<String> = Vec::new();

    for prop in &el.props {
        let name = &source[prop.start as usize..prop.name_end as usize];

        if !prop.is_directive {
            // Static attribute: name="value" or boolean attribute
            // Skip class and style (Vue handles these specially)
            if name.eq_ignore_ascii_case("class") || name.eq_ignore_ascii_case("style") {
                continue;
            }
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                // JSON-stringify the value
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                entries.push(format!("\"{}\": \"{}\"", name, escaped));
            } else {
                // Boolean attribute (no value)
                entries.push(format!("\"{}\": true", name));
            }
        } else {
            // Directive
            let (arg_start, arg_end) = match (prop.arg_start, prop.arg_end) {
                (Some(s), Some(e)) => (s, e),
                _ => continue, // no argument (e.g., v-show with no arg → skip for props)
            };

            if name == ":" || name == "v-bind" {
                // Dynamic bind: :name="expr" → "name": expr
                let arg_name = &source[arg_start as usize..arg_end as usize];
                // Skip class and style
                if arg_name.eq_ignore_ascii_case("class") || arg_name.eq_ignore_ascii_case("style")
                {
                    continue;
                }
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    // If the expression is a bare identifier matching a prop name,
                    // prefix with `__props.` since props aren't destructured at
                    // script scope (where Comp functions live).
                    let resolved = resolve_all_prop_refs_in_expr(expr, prop_names);
                    entries.push(format!("\"{}\": {}", arg_name, resolved));
                }
            } else if name == "@" || name == "v-on" {
                // Event handler: @name="expr" → "onName": () => {}
                // We use a placeholder function since only the key matters for
                // getRootComponentPassedProps (used in Omit for $attrs typing).
                let arg_name = &source[arg_start as usize..arg_end as usize];
                let on_name = event_to_jsx_name(arg_name);
                entries.push(format!("\"{}\": () => {{}}", on_name));
            }
            // Other directives (v-show, v-model, etc.) are not included in props
        }
    }

    if entries.is_empty() {
        "{}".to_string()
    } else {
        format!("{{{}}}", entries.join(", "))
    }
}

/// Resolve ALL prop name identifiers in an expression, replacing each bare prop
/// name with `__props.propName`. Used for Comp function condition guards where the
/// raw template expression may contain multiple prop references (e.g., `showBoard || isEditing`).
///
/// Uses OXC expression parser for correct handling of:
/// - Object shorthand `{ flag }` — NOT prefixed (it's both key and value)
/// - Computed property keys `{ [flag]: 1 }` — prefixed
/// - Non-computed property keys `{ flag: val }` — NOT prefixed
/// - Arrow function params `(flag) => flag` — shadows prop, NOT prefixed
/// - Member expressions `flag.value` — only root is prefixed
pub(super) fn resolve_all_prop_refs_in_expr(
    expr: &str,
    prop_names: &rustc_hash::FxHashSet<&str>,
) -> String {
    if prop_names.is_empty() || expr.is_empty() {
        return expr.to_string();
    }

    let alloc = oxc_allocator::Allocator::new();
    let parser = verter_parser::oxc_parse::Parser::new(&alloc, expr, oxc_span::SourceType::tsx());
    let parsed = match parser.parse_expression() {
        Ok(parsed) => parsed,
        Err(_) => return expr.to_string(), // fallback: return unchanged on parse error
    };

    // Collect byte offsets where "__props." should be inserted
    let mut insertions: Vec<u32> = Vec::new();
    collect_prop_refs(
        &parsed,
        prop_names,
        &mut insertions,
        &rustc_hash::FxHashSet::default(),
    );

    if insertions.is_empty() {
        return expr.to_string();
    }

    insertions.sort_unstable();
    insertions.dedup();

    let mut result = String::with_capacity(expr.len() + insertions.len() * 8);
    let mut last = 0usize;
    for offset in &insertions {
        let off = *offset as usize;
        result.push_str(&expr[last..off]);
        result.push_str("__props.");
        last = off;
    }
    result.push_str(&expr[last..]);
    result
}

/// Recursively walk an OXC expression, collecting byte offsets of identifiers
/// that match `prop_names` and should be prefixed with `__props.`.
fn collect_prop_refs(
    expr: &oxc_ast::ast::Expression,
    prop_names: &rustc_hash::FxHashSet<&str>,
    out: &mut Vec<u32>,
    shadowed: &rustc_hash::FxHashSet<&str>,
) {
    use oxc_ast::ast::*;

    match expr {
        Expression::Identifier(ident) => {
            if prop_names.contains(ident.name.as_str()) && !shadowed.contains(ident.name.as_str()) {
                out.push(ident.span.start);
            }
        }

        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        if p.computed {
                            // Computed key: `{ [flag]: 1 }` — prefix identifiers in key
                            collect_prop_refs_in_property_key(&p.key, prop_names, out, shadowed);
                        }
                        if p.shorthand {
                            // Shorthand `{ flag }` — skip entirely (it's both key and value)
                        } else {
                            collect_prop_refs(&p.value, prop_names, out, shadowed);
                        }
                    }
                    ObjectPropertyKind::SpreadProperty(s) => {
                        collect_prop_refs(&s.argument, prop_names, out, shadowed);
                    }
                }
            }
        }

        Expression::ArrayExpression(array) => {
            for elem in &array.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(s) => {
                        collect_prop_refs(&s.argument, prop_names, out, shadowed);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(e) = elem.as_expression() {
                            collect_prop_refs(e, prop_names, out, shadowed);
                        }
                    }
                }
            }
        }

        Expression::BinaryExpression(binary) => {
            collect_prop_refs(&binary.left, prop_names, out, shadowed);
            collect_prop_refs(&binary.right, prop_names, out, shadowed);
        }

        Expression::LogicalExpression(logical) => {
            collect_prop_refs(&logical.left, prop_names, out, shadowed);
            collect_prop_refs(&logical.right, prop_names, out, shadowed);
        }

        Expression::UnaryExpression(unary) => {
            collect_prop_refs(&unary.argument, prop_names, out, shadowed);
        }

        Expression::ConditionalExpression(cond) => {
            collect_prop_refs(&cond.test, prop_names, out, shadowed);
            collect_prop_refs(&cond.consequent, prop_names, out, shadowed);
            collect_prop_refs(&cond.alternate, prop_names, out, shadowed);
        }

        Expression::CallExpression(call) => {
            collect_prop_refs(&call.callee, prop_names, out, shadowed);
            for arg in &call.arguments {
                match arg {
                    Argument::SpreadElement(s) => {
                        collect_prop_refs(&s.argument, prop_names, out, shadowed);
                    }
                    _ => {
                        if let Some(e) = arg.as_expression() {
                            collect_prop_refs(e, prop_names, out, shadowed);
                        }
                    }
                }
            }
        }

        Expression::NewExpression(new_expr) => {
            collect_prop_refs(&new_expr.callee, prop_names, out, shadowed);
            for arg in &new_expr.arguments {
                match arg {
                    Argument::SpreadElement(s) => {
                        collect_prop_refs(&s.argument, prop_names, out, shadowed);
                    }
                    _ => {
                        if let Some(e) = arg.as_expression() {
                            collect_prop_refs(e, prop_names, out, shadowed);
                        }
                    }
                }
            }
        }

        // Member expressions — only visit the object (root), not the property
        Expression::StaticMemberExpression(m) => {
            collect_prop_refs(&m.object, prop_names, out, shadowed);
        }
        Expression::ComputedMemberExpression(m) => {
            collect_prop_refs(&m.object, prop_names, out, shadowed);
            collect_prop_refs(&m.expression, prop_names, out, shadowed);
        }
        Expression::PrivateFieldExpression(m) => {
            collect_prop_refs(&m.object, prop_names, out, shadowed);
        }

        Expression::TemplateLiteral(template) => {
            for expr in &template.expressions {
                collect_prop_refs(expr, prop_names, out, shadowed);
            }
        }

        Expression::TaggedTemplateExpression(tagged) => {
            collect_prop_refs(&tagged.tag, prop_names, out, shadowed);
            for expr in &tagged.quasi.expressions {
                collect_prop_refs(expr, prop_names, out, shadowed);
            }
        }

        Expression::ArrowFunctionExpression(arrow) => {
            // Collect parameter names that shadow props
            let mut inner_shadowed = shadowed.clone();
            for param in &arrow.params.items {
                collect_binding_pattern_names(&param.pattern, &mut inner_shadowed);
            }
            if let Some(expression) = arrow.get_expression() {
                // Expression body: `(x) => x + 1`
                collect_prop_refs(expression, prop_names, out, &inner_shadowed);
            } else if let Some(body) = arrow.get_function_body() {
                // Block body — not typical for template expressions, but handle it
                for stmt in &body.statements {
                    if let oxc_ast::ast::Statement::ExpressionStatement(es) = stmt {
                        collect_prop_refs(&es.expression, prop_names, out, &inner_shadowed);
                    } else if let oxc_ast::ast::Statement::ReturnStatement(rs) = stmt {
                        if let Some(arg) = &rs.argument {
                            collect_prop_refs(arg, prop_names, out, &inner_shadowed);
                        }
                    }
                }
            }
        }

        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_prop_refs(expr, prop_names, out, shadowed);
            }
        }

        Expression::AssignmentExpression(assign) => {
            collect_prop_refs(&assign.right, prop_names, out, shadowed);
        }

        Expression::AwaitExpression(a) => {
            collect_prop_refs(&a.argument, prop_names, out, shadowed);
        }

        Expression::YieldExpression(y) => {
            if let Some(arg) = &y.argument {
                collect_prop_refs(arg, prop_names, out, shadowed);
            }
        }

        Expression::ParenthesizedExpression(p) => {
            collect_prop_refs(&p.expression, prop_names, out, shadowed);
        }

        Expression::TSNonNullExpression(ts) => {
            collect_prop_refs(&ts.expression, prop_names, out, shadowed);
        }

        Expression::TSAsExpression(ts) => {
            collect_prop_refs(&ts.expression, prop_names, out, shadowed);
        }

        Expression::TSSatisfiesExpression(ts) => {
            collect_prop_refs(&ts.expression, prop_names, out, shadowed);
        }

        Expression::TSTypeAssertion(ts) => {
            collect_prop_refs(&ts.expression, prop_names, out, shadowed);
        }

        // Literals, this, super, etc. — no prop refs
        _ => {}
    }
}

/// Collect prop refs from a property key (only for computed keys).
fn collect_prop_refs_in_property_key(
    key: &oxc_ast::ast::PropertyKey,
    prop_names: &rustc_hash::FxHashSet<&str>,
    out: &mut Vec<u32>,
    shadowed: &rustc_hash::FxHashSet<&str>,
) {
    match key {
        oxc_ast::ast::PropertyKey::StaticIdentifier(ident) => {
            // In a computed key context, the identifier IS an expression
            if prop_names.contains(ident.name.as_str()) && !shadowed.contains(ident.name.as_str()) {
                out.push(ident.span.start);
            }
        }
        _ => {
            if let Some(expr) = key.as_expression() {
                collect_prop_refs(expr, prop_names, out, shadowed);
            }
        }
    }
}

/// Collect binding names from a destructuring/binding pattern into a set.
fn collect_binding_pattern_names<'a>(
    pattern: &'a oxc_ast::ast::BindingPattern<'a>,
    names: &mut rustc_hash::FxHashSet<&'a str>,
) {
    use oxc_ast::ast::BindingPattern;
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            names.insert(ident.name.as_str());
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_pattern_names(&prop.value, names);
            }
            if let Some(rest) = &obj.rest {
                collect_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_binding_pattern_names(elem, names);
            }
            if let Some(rest) = &arr.rest {
                collect_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_binding_pattern_names(&assign.left, names);
        }
    }
}

// camelize_event removed — use event_to_jsx_name from super instead

/// Emit a single Comp{offset} function for an element.
///
/// When `comp_scopes` indicates the tag comes from a v-slot or v-for scope,
/// the function reconstructs the type through the parent's instantiated type
/// rather than referencing the tag name directly (which isn't in top-level scope).
///
/// Under a condition (`region`), an element function's body reads nothing the
/// conditions narrow, so a constant guard only contributes `| null` to its
/// return type. A component function's tag and props read narrowed values:
/// they are captured in the branch's block of the [`ComponentFlow`], and the
/// function reads them back through the branch's navigator.
#[allow(clippy::too_many_arguments)]
fn emit_comp_function_for_element(
    buf: &mut String,
    gs: &str,
    _gn: &str,
    el: &ElementNode,
    source: &str,
    offset: u32,
    region: Option<FlowRegionRef>,
    flow: &mut ComponentFlow,
    comp_scopes: &[CompScope],
    is_jsx: bool,
    props_literal: &str,
) {
    use std::fmt::Write;

    let raw_tag = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
    let guard = if region.is_some() {
        "\n  if(!___VERTER___flowBranch) return null;"
    } else {
        ""
    };

    // <component :is="..."> — the component type is dynamic, so we can't
    // reference a `component` variable. Emit a function that returns `unknown`
    // so that getRootComponent/void chains still resolve.
    if raw_tag == "component" {
        write!(
            buf,
            "\nfunction {P}Comp{offset}{gs}() {{{guard}\
             \n  return {{}} as unknown;\
             \n}}",
            P = PREFIX,
            offset = offset,
            gs = gs,
            guard = guard,
        )
        .expect("write to String is infallible");
        return;
    }

    // Component tag names in templates use case-insensitive matching against imports.
    // `<card>` resolves to `Card`, `<a-switch>` to `ASwitch`. PascalCase-convert
    // for component tags; HTML elements keep their raw lowercase name for
    // HTMLElementTagNameMap lookup.
    let pascal_tag = to_pascal_case(raw_tag);
    let tag_name: &str = if el.tag_type == TagType::Component {
        &pascal_tag
    } else {
        raw_tag
    };

    match el.tag_type {
        TagType::Element => {
            // For HTML elements, return the plain element type without props enhancement.
            // useTemplateRef should resolve to e.g. HTMLSpanElement, not HTMLSpanElement & { onClick: ... }
            if is_jsx {
                write!(
                    buf,
                    "\nfunction {P}Comp{offset}{gs}() {{{guard}\
                     \n  return /** @type {{HTMLElementTagNameMap[\"{tag}\"]}} */ ({{}});\
                     \n}}",
                    P = PREFIX,
                    offset = offset,
                    gs = gs,
                    guard = guard,
                    tag = tag_name,
                )
                .expect("write to String is infallible");
            } else {
                write!(
                    buf,
                    "\nfunction {P}Comp{offset}{gs}() {{{guard}\
                     \n  return {{}} as HTMLElementTagNameMap[\"{tag}\"];\
                     \n}}",
                    P = PREFIX,
                    offset = offset,
                    gs = gs,
                    guard = guard,
                    tag = tag_name,
                )
                .expect("write to String is infallible");
            }
        }
        TagType::Component => {
            // The statements that bind the tag and props before the component
            // is instantiated, and the instantiated tag and props.
            let (bindings, tag, props) = match find_scope_for_tag(tag_name, comp_scopes) {
                // Reconstruct the type through the parent's instantiated slot
                // type. The parent Comp function instantiates the parent
                // component with its actual props, so TypeScript infers generics
                // correctly. We drill into $slots to extract the slot prop type,
                // then destructure to get the specific binding.
                Some(CompScope::VSlot {
                    parent_comp_offset,
                    slot_name,
                    params_expr,
                    ..
                }) => (
                    format!(
                        "\n  type __Parent = ReturnType<typeof {P}Comp{parent_comp_offset}>;\
                         \n  type __SlotFn = NonNullable<__Parent['$slots']['{slot_name}']>;\
                         \n  type __SlotProps = __SlotFn extends (...args: infer A) => any ? A[0] : {{}};\
                         \n  const {params_expr} = {{}} as __SlotProps;",
                        P = PREFIX,
                    ),
                    tag_name,
                    props_literal,
                ),
                // Reconstruct the type from the v-for iterable's element type.
                Some(CompScope::VFor { iterable_expr, .. }) => (
                    format!("\n  const {tag_name} = {{}} as (typeof {iterable_expr})[number];"),
                    tag_name,
                    props_literal,
                ),
                None => (String::new(), tag_name, props_literal),
            };
            match region {
                Some(region) => flow.add_component(region, offset, &bindings, tag, props),
                None => write!(
                    buf,
                    "\nfunction {P}Comp{offset}{gs}() {{{bindings}\
                     \n  return {P}instantiateComponent({tag}, {props});\
                     \n}}",
                    P = PREFIX,
                )
                .expect("write to String is infallible"),
            }
        }
        TagType::SlotOutlet | TagType::Template => {
            // Skip <slot> and <template> wrappers
        }
    }
}
