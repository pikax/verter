//! Publication-demand + Shallow decl-body-lowering discriminator suite.
//!
//! Pins two architectural invariants of the shared resolver:
//!
//! 1. **Carrier-preserving Shallow decl-body lowering** — under
//!    `ProjectionMode::Shallow` (as under `Navigate` / `Skeleton`),
//!    decl-body lowering interns `DeclRef` / `InstantiationRef`
//!    carriers for member-value type references — including ALL
//!    builtin utilities — and never executes `ResolveDecl` /
//!    `Instantiate` eagerly. Eager lowering-time execution is
//!    `Expanded` / `Identity` only. Materialisation enters
//!    exclusively through the demand points (PathWalker hops, the
//!    shallow-surface synthesiser's carrier unwrap, closed
//!    object-filter surface reads, the relation/conditional oracle).
//!
//! 2. **Publication demands Navigate, never Expanded** — every
//!    projector / registry / materialiser entrance on the
//!    component-meta publication pipeline dispatches `Published`
//!    projection contexts at `Navigate` (terminal hop in the
//!    caller's mode); a full `get_component_meta` records ZERO
//!    `Published(Expanded)` projection contexts on the dispatch
//!    stream. The request-mode axis
//!    (`compute_component_meta_state_inner`) is a REQUEST property,
//!    not a projection context, and is out of this contract's
//!    scope.
//!
//! 3. **`typeof` lowers at the requested demand** — the `TypeOf`
//!    query carries the caller's projection-reduction context and
//!    `build_typeof` lowers the value's annotation / object shape /
//!    signatures / enum surface at that demand, so a Skeleton /
//!    Navigate / Shallow caller crossing a `typeof`-typed value never
//!    detonates an Expanded lowering of the value's declaration
//!    graph at build time.
//!
//! Fixtures are vendored-in-memory only (Testing-Hermeticity). The
//! mutually-referential generic web below is corpus-shaped (the
//! TanStack `Table.vue` decl-graph class): member values
//! instantiate sibling interfaces with VARIED argument shapes
//! (`T`, `T[]`, `Partial<X<T>>`, …) so eager member-value lowering
//! cannot collapse onto a few memo entries — an eager lowerer
//! generates unboundedly many distinct instantiation keys and trips
//! any finite projection budget, while carrier-preserving lowering
//! touches only the demanded heritage spine.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::capture_token::{CaptureToken, DispatchEntry};
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, ReductionDemand, SemanticQueryKey,
};
use crate::types::HostConfig;
use crate::VerterHost;
use verter_type_expr::TypeExpr;

/// Tight per-test projection budget: small enough that the eager
/// member-value lowering storm over [`WEB_TYPES_TS`] trips it, large
/// enough that the demanded heritage-spine materialisation (tens of
/// dispatches) never approaches it.
const TIGHT_PROJECTION_BUDGET: usize = 200;

#[allow(deprecated)]
fn make_workspace_project_config(root: &str) -> verter_workspace::VfsProjectConfig {
    verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::ProjectMembership::MatchAll,
    }
}

/// Hermetic host over a `MemoryWorkspace` with a configured project
/// graph and an explicit `projection_op_budget` (0 = the armed
/// default).
fn build_host_with_budget(files: &[(&str, &str)], projection_op_budget: usize) -> Arc<VerterHost> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let project_config = make_workspace_project_config("/workspace");
    #[allow(deprecated)]
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        project_config.clone(),
    ]));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ide_project = project_config.to_ide_project_config();
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            projection_op_budget,
            ..HostConfig::default()
        },
        ws_access,
        scheduler_config,
    );
    host.configure_projects(vec![ide_project]);
    Arc::new(host)
}

fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    build_host_with_budget(files, 0)
}

/// A dispatched key's `Published(Expanded)` projection context, if
/// any. Only the context-bearing projection families carry the
/// publication demand axis; `ResolveMacroPayload`'s request-mode
/// axis and the bare-mode sugar variants are NOT projection
/// contexts.
fn key_is_published_expanded(key: &SemanticQueryKey) -> bool {
    let ctx: Option<ProjectionReductionContext> = match key {
        SemanticQueryKey::Instantiate { context, .. } => Some(context.projection_reduction()),
        SemanticQueryKey::TypeOf { context, .. } => Some(context.projection_reduction),
        SemanticQueryKey::KeyOf { context, .. }
        | SemanticQueryKey::MappedType { context, .. }
        | SemanticQueryKey::ProjectPath { context, .. } => Some(*context),
        _ => None,
    };
    ctx.is_some_and(|c| {
        c.demand == ReductionDemand::Published && c.mode == ProjectionMode::Expanded
    })
}

fn published_expanded_dispatches(log: &[DispatchEntry]) -> Vec<String> {
    log.iter()
        .filter(|e| key_is_published_expanded(&e.key))
        .map(|e| format!("{:?}", e.key))
        .collect()
}

/// Classifier self-test: every context-bearing projection family —
/// including the demand-bearing `TypeOf` — must be visible to the
/// `Published(Expanded)` detector, or the zero-`Published(Expanded)`
/// dispatch-log gates in this file go blind to a regression in that
/// family (a `TypeOf { context: Published(Expanded) }` storm would pass
/// the typeof-lane guards unreported).
#[test]
fn published_expanded_classifier_sees_every_context_bearing_family() {
    use crate::semantic_query::{
        InstantiateContext, ScopeId, SemanticNodeId, TypeOfContext, ValueRootKey,
        ValueRootSlotIdentity,
    };

    let published_expanded = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let published_navigate = ProjectionReductionContext::published(ProjectionMode::Navigate);
    let type_of = |ctx: ProjectionReductionContext| SemanticQueryKey::TypeOf {
        value_root: ValueRootSlotIdentity::new(
            ValueRootKey {
                scope: ScopeId::file(Arc::from("/classifier/value.ts")),
                name: Arc::from("sample"),
            },
            0,
            Default::default(),
            Default::default(),
        ),
        context: TypeOfContext::new(ctx, Default::default()),
    };
    assert!(
        key_is_published_expanded(&type_of(published_expanded)),
        "a TypeOf key carrying Published(Expanded) demand MUST be reported as an \
         offending key — the typeof-lane guards depend on this classifier"
    );
    assert!(
        !key_is_published_expanded(&type_of(published_navigate)),
        "a Navigate-demand TypeOf key must NOT be flagged"
    );

    let instantiate = SemanticQueryKey::Instantiate {
        base: crate::semantic_query::DeclIdentity::synthetic("X").to_type_slot_unscoped(),
        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: InstantiateContext::non_file_for_tests(published_expanded, Default::default()),
    };
    assert!(key_is_published_expanded(&instantiate));
    let project_path = SemanticQueryKey::ProjectPath {
        base: SemanticNodeId(0),
        path: Arc::from(Vec::new().into_boxed_slice()),
        context: published_expanded,
    };
    assert!(key_is_published_expanded(&project_path));
}

fn cold_instantiate_dispatches(log: &[DispatchEntry]) -> usize {
    log.iter()
        .filter(|e| !e.hit && matches!(e.key, SemanticQueryKey::Instantiate { .. }))
        .count()
}

// ===========================================================================
// Shared fixtures
// ===========================================================================

/// Corpus-shaped mutually-referential generic web (the TanStack
/// `Table.vue` decl-graph class), package-backed. The `T[]` /
/// `Partial<…>` / second-type-parameter argument variations are
/// load-bearing: they defeat instantiation-key memoisation so an
/// EAGER member-value lowerer compounds distinct keys transitively
/// (`Col<T, V>` → `Header<T[], V>` → `Opts<T[][]>` → …) and trips
/// any finite budget, while carrier-preserving lowering never
/// descends member values at all.
const WEB_TYPES_TS: &str = r#"export type RowData = unknown;
export type Updater<X> = X | ((old: X) => X);
export interface CoreOptions<T extends RowData> {
  data: T[];
  state: TableState<T>;
  renderFallback: RowModel<T>;
  onStateChange: (updater: Updater<TableState<T>>) => void;
  defaultColumn?: Partial<ColumnDef<T, unknown>>;
  getCoreRowModel: (table: Table<T>) => () => RowModel<T>;
  mergeOptions?: (a: Opts<T>, b: Partial<Opts<T>>) => Opts<T>;
  meta?: TableMeta<T>;
  features: Feat<T>;
  rows: Row<T[]>;
}
export interface Table<T> {
  options: Opts<T>;
  initialState: TableState<T>;
  groups: HeaderGroup<T>[];
  rowModel: RowModel<T[]>;
  features: Feat<T>;
}
export interface RowModel<T> {
  rows: Row<T>[];
  flat: Row<T[]>[];
  byId: Table<T>;
}
export interface Row<T> {
  cells: Cell<T, unknown>[];
  table: Table<T>;
  parent?: Row<T[]>;
  model: RowModel<T>;
}
export interface Cell<T, V> {
  column: Col<T, V>;
  row: Row<T>;
  ctx: Header<T, V>;
  value: V;
}
export interface Col<T, V> {
  header?: Header<T[], V>;
  cell?: (ctx: Opts<T>) => Col<T, V>;
  meta?: Feat<T>;
  group: Col<T[], V>;
  def: ColumnDef<T, V>;
}
export interface ColumnDef<T, V> {
  col: Col<T, V>;
  cellRender?: (cell: Cell<T, V>) => Header<T, V>;
  table: Table<T>;
  state: TableState<T[]>;
}
export interface Header<T, V> {
  column: Col<T, V>;
  table: Table<T>;
  render?: (o: Opts<T[]>) => Col<T, V>;
  group: HeaderGroup<T>;
}
export interface HeaderGroup<T> {
  headers: Header<T, unknown>[];
  depthRows: Row<T[]>[];
  table: Table<T[]>;
}
export interface Opts<T> {
  core: CoreOptions<T>;
  columns: Col<T, unknown>[];
  headers: Header<T[], unknown>[];
  state: TableState<T>;
  features: Feat<T[]>;
}
export interface TableState<T> {
  current: Opts<T>;
  snapshot?: Partial<TableState<T[]>>;
  features: Feat<T>;
  cols: Col<T, unknown>[];
  groups: HeaderGroup<T>[];
}
export interface Feat<T> {
  options: Opts<T[]>;
  state: TableState<T>;
  cols: Col<T, unknown>[];
  table: Table<T>;
  defs: ColumnDef<T[], unknown>[];
}
export interface TableMeta<T> {
  table: Table<T>;
  rows: Row<T>[];
  state: TableState<T[]>;
}
"#;

/// SFC mirroring the corpus `Table.vue` macro shape: a non-setup
/// `<script lang="ts">` block exporting a two-level heritage chain
/// over a package-backed CLOSED `Omit` (`T` confined to member-value
/// positions keeps the key domain closed — the L1 rule), consumed by
/// `withDefaults(defineProps<TableProps<T>>(), …)` under a
/// constrained SFC generic.
const WEB_SFC_VUE: &str = r#"<script lang="ts">
import type { CoreOptions, RowData, TableMeta, Col } from 'ttable';

export type TableData = RowData;
export type TableColumn<T extends TableData, D = unknown> = Col<T, D>;

export interface TableOptions<T extends TableData = TableData> extends Omit<CoreOptions<T>, 'data' | 'state' | 'onStateChange' | 'renderFallback'> {
  state?: CoreOptions<T>['state'];
  onStateChange?: CoreOptions<T>['onStateChange'];
}

export interface TableProps<T extends TableData = TableData> extends TableOptions<T> {
  as?: any;
  data?: T[];
  columns?: TableColumn<T>[];
  caption?: string;
  meta?: TableMeta<T>;
}
</script>

<script setup lang="ts" generic="T extends TableData">
const _props = withDefaults(defineProps<TableProps<T>>(), {
  caption: 'table'
});
</script>
<template><div /></template>
"#;

// ===========================================================================
// #1 — THE A discriminator
// ===========================================================================

/// Decl-body lowering keeps member-value type references as typed-IR
/// carriers under Shallow demand: resolving the web-heritage SFC
/// publishes the inherited key NAMES (over-stopping guard), keeps
/// member VALUES as `Ref` / instantiation-shaped carriers, performs
/// a bounded number of cold `Instantiate` dispatches (the demanded
/// heritage spine only), trips no budget, and warms.
///
/// An eager Shallow member-value lowerer recursively instantiates
/// the web with compounding argument shapes, trips the tight budget,
/// and fails the no-suppress + ceiling assertions.
#[test]
fn decl_body_lowering_keeps_member_value_refs_as_carriers() {
    let host = build_host_with_budget(
        &[
            (
                "/workspace/node_modules/ttable/package.json",
                r#"{ "name": "ttable", "types": "./index.d.ts" }"#,
            ),
            ("/workspace/node_modules/ttable/index.d.ts", WEB_TYPES_TS),
            ("/workspace/src/Comp.vue", WEB_SFC_VUE),
        ],
        TIGHT_PROJECTION_BUDGET,
    );

    let guard = CaptureToken::start_for_query("a_discriminator_cold");
    let resolved = host.get_component_meta_with_resolution("/workspace/src/Comp.vue");
    let snapshot = guard.end();
    let (meta, resolution) = resolved.expect("web-heritage SFC must resolve");

    assert!(
        !resolution.synthesis_should_suppress,
        "carrier-preserving Shallow lowering must complete WITHOUT a budget trip \
         (synthesis_should_suppress=true means the eager member-value lowering storm \
         tripped the {TIGHT_PROJECTION_BUDGET}-op fuse)"
    );

    // Over-stopping guard: the heritage keys ENUMERATE (Omit over a
    // closed key domain materialises the non-excluded keys
    // path-precisely) and the excluded key stays out.
    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for key in [
        "caption",
        "defaultColumn",
        "mergeOptions",
        "getCoreRowModel",
        "features",
        "rows",
    ] {
        assert!(
            names.contains(&key),
            "heritage key `{key}` must publish (got {names:?}) — carrier preservation \
             must NOT over-stop the closed-Omit heritage enumeration"
        );
    }
    assert!(
        !names.contains(&"renderFallback"),
        "Omit-excluded key `renderFallback` must NOT publish (got {names:?})"
    );

    // Member VALUES stay typed-IR carriers (structural assertion —
    // never string matching): the inherited `features` member's
    // published value is the `Feat<…>` reference carrier, not an
    // eagerly materialised object surface.
    let features = meta
        .props
        .iter()
        .find(|p| p.name == "features")
        .expect("features prop present");
    assert!(
        matches!(
            &features.type_expr,
            TypeExpr::Ref { name, .. } if name.as_ref() == "Feat"
        ),
        "inherited member value must publish as the `Feat` reference carrier, got {:?}",
        features.type_expr
    );

    // Lowering-eagerness ceiling: the demanded heritage spine costs a
    // bounded number of COLD `Instantiate` dispatches. The eager
    // member-value storm compounds distinct instantiation keys and
    // blows far past this ceiling before tripping the fuse.
    let cold = cold_instantiate_dispatches(&snapshot.dispatch_log);
    assert!(
        cold < 40,
        "cold `Instantiate` dispatches must stay under the heritage-spine ceiling \
         (got {cold}; eager member-value decl-body lowering compounds the web)"
    );

    // Warm pass collapses: a second resolve re-dispatches almost
    // nothing cold (final-result cache + validated semantic memo).
    let warm_guard = CaptureToken::start_for_query("a_discriminator_warm");
    let warm = host.get_component_meta_with_resolution("/workspace/src/Comp.vue");
    let warm_snapshot = warm_guard.end();
    let (_, warm_resolution) = warm.expect("warm resolve must succeed");
    assert!(!warm_resolution.synthesis_should_suppress);
    let warm_cold = cold_instantiate_dispatches(&warm_snapshot.dispatch_log);
    assert!(
        warm_cold <= 8,
        "warm pass must collapse cold instantiate dispatches (got {warm_cold})"
    );
}

// ===========================================================================
// #2 — over-stopping guard (closed Omit heritage, package-backed base)
// ===========================================================================

const PKG_WIDGETS_DTS: &str = r#"export interface LabelOpts {
  text: string;
  size: number;
}
export interface WidgetBase {
  label?: LabelOpts;
  count: number;
  secret: string;
}
"#;

const PKG_SFC_VUE: &str = r#"<script setup lang="ts">
import type { WidgetBase } from 'widgets';
interface Props extends Omit<WidgetBase, 'secret'> {
  own?: boolean;
}
defineProps<Props>();
</script>
<template><div /></template>
"#;

/// Closed `Omit` heritage — including over a package-backed base —
/// still ENUMERATES the inherited keys path-precisely; the excluded
/// key stays out; resolution completes without a budget trip. This
/// is the carrier-unwrap chain pin (shallow surface synthesiser →
/// `InstantiationRef` unwrap → builtin Omit materialisation →
/// heritage-arm role stamping).
#[test]
fn closed_omit_heritage_publishes_keys_with_shallow_values() {
    let host = build_host(&[
        (
            "/workspace/node_modules/widgets/package.json",
            r#"{ "name": "widgets", "types": "./index.d.ts" }"#,
        ),
        (
            "/workspace/node_modules/widgets/index.d.ts",
            PKG_WIDGETS_DTS,
        ),
        ("/workspace/src/Widget.vue", PKG_SFC_VUE),
    ]);

    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/Widget.vue")
        .expect("package-backed Omit heritage SFC must resolve");
    assert!(!resolution.synthesis_should_suppress);

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for key in ["own", "label", "count"] {
        assert!(
            names.contains(&key),
            "inherited key `{key}` must publish through the closed package-backed \
             Omit heritage (got {names:?})"
        );
    }
    assert!(
        !names.contains(&"secret"),
        "Omit-excluded key `secret` must NOT publish (got {names:?})"
    );
}

// ===========================================================================
// #3 — path-precision guard
// ===========================================================================

const PATH_TYPES_TS: &str = r#"export interface Inner {
  data: string[];
  tag: boolean;
}
export interface Wrap {
  core: Inner;
  label: string;
}
"#;

const PATH_SFC_VUE: &str = r#"<script setup lang="ts">
import type { Wrap } from './pathtypes';
defineProps<{ picked: Wrap['core']['data'] }>();
</script>
<template><div /></template>
"#;

/// `Foo['a']['b']` materialises under path demand: intermediate hops
/// navigate, the terminal hop materialises in the caller's mode. The
/// PathWalker intermediate/terminal contract is unchanged by
/// carrier-preserving lowering.
#[test]
fn tan_stack_selected_member_still_materialises_when_projected() {
    let host = build_host(&[
        ("/workspace/src/pathtypes.ts", PATH_TYPES_TS),
        ("/workspace/src/Picked.vue", PATH_SFC_VUE),
    ]);

    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/Picked.vue")
        .expect("path-projection SFC must resolve");
    assert!(!resolution.synthesis_should_suppress);

    let picked = meta
        .props
        .iter()
        .find(|p| p.name == "picked")
        .expect("picked prop present");
    // `Opts<string>['core']['data']` terminal is `T[]` with
    // `T = string` — a concrete array of string.
    assert!(
        matches!(
            &picked.type_expr,
            TypeExpr::Array { element, .. }
                if matches!(element.as_ref(), TypeExpr::Primitive(p) if format!("{p:?}") == "String")
        ),
        "path-projected terminal must materialise to string[], got {:?}",
        picked.type_expr
    );
}

// ===========================================================================
// #4 — THE B discriminator
// ===========================================================================

const SLOTS_TYPES_TS: &str = r#"import type { Opts, TableState } from 'ttable';
export interface ChatSlots {
  header?: (props: { id: string }) => string[];
  footer?: (props: { ok: boolean }) => string[];
}
export type MB<T> = T extends { meta: infer M } ? Opts<M> : TableState<T>;
export type ChatVueSlots<T> = { [K in keyof ChatSlots]?: NonNullable<ChatSlots[K]> extends (p: infer P) => string[] ? (p: P & { message: MB<T> }) => string[] : never };
"#;

const SLOTS_SFC_VUE: &str = r#"<script setup lang="ts" generic="T">
import type { ChatSlots, MB } from './slots';
defineSlots<{ [K in keyof ChatSlots]?: NonNullable<ChatSlots[K]> extends (p: infer P) => string[] ? (p: P & { message: MB<T> }) => string[] : never }>();
</script>
<template><div /></template>
"#;

/// Mapped slots surface with CLOSED keys and an open-conditional
/// VALUE (`MB<T>` over the unbound SFC generic): the slot names
/// enumerate path-precisely; the `message` binding publishes as a
/// deferred carrier (the `MB` reference / conditional shell — NOT a
/// both-branch union materialised through the web); no budget trip;
/// the warm pass collapses.
///
/// An Expanded publication pipeline materialises the open
/// conditional through both branches (`Opts<M>` / `State<T>` drag
/// the compounding web) and trips the tight budget.
#[test]
fn mapped_closed_keys_open_conditional_value_publishes_deferred_carrier() {
    let host = build_host_with_budget(
        &[
            (
                "/workspace/node_modules/ttable/package.json",
                r#"{ "name": "ttable", "types": "./index.d.ts" }"#,
            ),
            ("/workspace/node_modules/ttable/index.d.ts", WEB_TYPES_TS),
            ("/workspace/src/slots.ts", SLOTS_TYPES_TS),
            ("/workspace/src/Chat.vue", SLOTS_SFC_VUE),
        ],
        TIGHT_PROJECTION_BUDGET,
    );

    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/Chat.vue")
        .expect("mapped slots SFC must resolve");
    assert!(
        !resolution.synthesis_should_suppress,
        "publication must NOT materialise the open-conditional mapped value through \
         Expanded demand (budget trip ⇒ the Expanded publication pipeline is alive)"
    );

    let slot_names: Vec<&str> = meta.slots.iter().map(|s| s.name.as_str()).collect();
    for key in ["header", "footer"] {
        assert!(
            slot_names.contains(&key),
            "mapped slot key `{key}` must enumerate (got {slot_names:?})"
        );
    }

    // The `message` binding's published value stays a deferred
    // carrier: a `Ref`-shaped reference to `MB` (or its conditional
    // shell) — structurally NOT a Union of both materialised
    // branches and NOT an object surface of the web's members.
    let header = meta
        .slots
        .iter()
        .find(|s| s.name == "header")
        .expect("header slot present");
    let message = header
        .bindings
        .iter()
        .find(|b| b.name == "message")
        .expect("message binding present");
    assert!(
        !matches!(&message.type_expr, TypeExpr::Union(_)),
        "open-conditional mapped value must NOT publish as a both-branch union, got {:?}",
        message.type_expr
    );
    assert!(
        !matches!(&message.type_expr, TypeExpr::Object(_)),
        "open-conditional mapped value must NOT publish as a materialised object \
         surface, got {:?}",
        message.type_expr
    );

    // Warm pass collapses cold dispatches.
    let warm_guard = CaptureToken::start_for_query("b_discriminator_warm");
    let warm = host.get_component_meta_with_resolution("/workspace/src/Chat.vue");
    let warm_snapshot = warm_guard.end();
    let (_, warm_resolution) = warm.expect("warm resolve must succeed");
    assert!(!warm_resolution.synthesis_should_suppress);
    let warm_cold = cold_instantiate_dispatches(&warm_snapshot.dispatch_log);
    assert!(
        warm_cold <= 8,
        "warm pass must collapse cold instantiate dispatches (got {warm_cold})"
    );
}

// ===========================================================================
// #5 — IndexedAccess terminal publishes shallow
// ===========================================================================

const THEME_TYPES_TS: &str = r#"export interface NestedCfg {
  depth: number;
}
export interface HeaderCfg {
  title: string;
  nested: NestedCfg;
}
export interface Theme {
  header: HeaderCfg;
}
"#;

const THEME_SFC_VUE: &str = r#"<script setup lang="ts">
import type { Theme } from './theme';
defineProps<{ cfg: Theme['header'] }>();
</script>
<template><div /></template>
"#;

/// `Theme['header']` publishes its terminal as the SHALLOW `HeaderCfg`
/// reference carrier. The path hops load path-precisely; the
/// terminal hop runs in the publication caller's mode (`Navigate`),
/// so the declaration reference survives to the published surface
/// and consumers re-resolve it on demand.
#[test]
fn indexed_access_terminal_publishes_shallow() {
    let host = build_host(&[
        ("/workspace/src/theme.ts", THEME_TYPES_TS),
        ("/workspace/src/Themed.vue", THEME_SFC_VUE),
    ]);

    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/Themed.vue")
        .expect("indexed-access SFC must resolve");
    assert!(!resolution.synthesis_should_suppress);

    let cfg = meta
        .props
        .iter()
        .find(|p| p.name == "cfg")
        .expect("cfg prop present");
    assert!(
        matches!(
            &cfg.type_expr,
            TypeExpr::Ref { name, type_arguments } if name.as_ref() == "HeaderCfg" && type_arguments.is_empty()
        ),
        "IndexedAccess terminal must publish the shallow `HeaderCfg` reference \
         carrier (consumers re-resolve on demand), got {:?}",
        cfg.type_expr
    );
}

// ===========================================================================
// #6 — behavioural architecture guard: publication never demands Expanded
// ===========================================================================

const GUARD_SFC_VUE: &str = r#"<script setup lang="ts">
import type { Theme } from './theme';
defineProps<{ cfg: Theme['header'] }>();
const m = defineModel<Theme['header']>();
</script>
<template><div /></template>
"#;

/// A full `get_component_meta` over a fixture exercising the
/// per-prop projector, the IndexedAccess terminal re-resolve, and
/// the model-payload projector records ZERO `Published(Expanded)`
/// projection contexts on the dispatch stream. Publication demand
/// is `Navigate` everywhere; `Expanded` materialisation is reserved
/// for genuine-Expanded consumers (typeinfo deep expansion).
#[test]
fn publication_routes_never_demand_expanded() {
    let host = build_host(&[
        ("/workspace/src/theme.ts", THEME_TYPES_TS),
        ("/workspace/src/Guarded.vue", GUARD_SFC_VUE),
    ]);

    let guard = CaptureToken::start_for_query("publication_demand_guard");
    let resolved = host.get_component_meta_with_resolution("/workspace/src/Guarded.vue");
    let snapshot = guard.end();
    let (_, resolution) = resolved.expect("guard SFC must resolve");
    assert!(!resolution.synthesis_should_suppress);

    let expanded = published_expanded_dispatches(&snapshot.dispatch_log);
    assert!(
        expanded.is_empty(),
        "publication recorded {} `Published(Expanded)` projection context(s); \
         publication demand is Navigate-only. Offending keys:\n{}",
        expanded.len(),
        expanded.join("\n")
    );
}

// ===========================================================================
// #6b — the relation/conditional oracle is a transit consumer
// ===========================================================================

/// The app-config conditional's check operand: `GetComponentAppConfig`'s
/// `A extends Record<U, Record<K, any>>` check is decided by the
/// relation/conditional oracle's Object-vs-Record arm
/// (`record_target_shape`), which must normalise the Record-shaped
/// target WITHOUT issuing `Published(*)`-context subqueries — the
/// oracle is an internal transit consumer.
const ORACLE_SCHEMA_TS: &str = r#"export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#;

/// `true` iff the key carries a `Published`-demand projection context
/// (ANY mode). The oracle-route gate below uses this stronger
/// predicate: a transit consumer must record no publication-demand
/// keys at all, not merely no `Published(Expanded)` ones.
fn key_is_published_any_mode(key: &SemanticQueryKey) -> bool {
    let ctx: Option<ProjectionReductionContext> = match key {
        SemanticQueryKey::Instantiate { context, .. } => Some(context.projection_reduction()),
        SemanticQueryKey::TypeOf { context, .. } => Some(context.projection_reduction),
        SemanticQueryKey::KeyOf { context, .. }
        | SemanticQueryKey::MappedType { context, .. }
        | SemanticQueryKey::ProjectPath { context, .. } => Some(*context),
        _ => None,
    };
    ctx.is_some_and(|c| c.demand == ReductionDemand::Published)
}

/// The relation/conditional oracle's Object-vs-Record arm
/// (`record_target_shape` + the source-side carrier unwrap) records
/// ZERO `Published(*)`-context semantic-query keys: every
/// normalisation, argument evaluation, and carrier materialisation it
/// demands is keyed under the `StructuralTransit` demand identity. A
/// relation check is an internal transit consumer, never a
/// publication route.
///
/// The probe drives the oracle directly through `relate_nodes` with
/// the app-config conditional's operands: the workspace-owned
/// `AppConfig` declaration carrier as the check side and the
/// substituted `Record<'ui', Record<'button', any>>` extends operand
/// riding an `InstantiationRef` carrier as the target side — the
/// exact shapes `GetComponentAppConfig<A, U, K>` hands the oracle.
/// The `Assignable` verdict doubles as the firing proof: Record-target
/// recognition (which gates the app-config merge) must keep working
/// under the transit demand.
#[test]
fn relation_oracle_record_target_normalisation_records_no_published_context() {
    use crate::semantic_query::{
        DeclIdentity, LiteralValue, PrimitiveKind, QueryResult, RelationResult, SemanticNodeData,
        SemanticQueryApi, SemanticQueryOutput,
    };

    let host = build_host(&[("/workspace/src/schema.ts", ORACLE_SCHEMA_TS)]);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&*host);
    let graph = std::sync::Arc::clone(host.project_type_store().semantic_graph());

    // Pre-capture setup: index the file and resolve the AppConfig
    // declaration carrier (the conditional's check operand). The
    // ResolveDecl warm-up stays OUTSIDE the capture window so the
    // captured stream is the oracle's work alone.
    host.shallow_file_state("/workspace/src/schema.ts")
        .expect("schema.ts must have shallow file state");
    let app_config = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(
        crate::semantic_query::ResolveDeclKey {
            scope: crate::semantic_query::ScopeId::file(Arc::from("/workspace/src/schema.ts")),
            name: Arc::from("AppConfig"),
        },
    )) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("AppConfig must resolve to a declaration carrier, got {other:?}"),
    };

    // The substituted extends operand: `Record<'ui', Record<'button',
    // any>>` riding `InstantiationRef` carriers over the builtin
    // `Record` identity — the shape carrier-preserving lowering
    // interns inside a decl body, exactly what `record_target_shape`'s
    // demand point materialises.
    let builtin_record_identity = || DeclIdentity {
        canonical_id: Arc::from("__builtin__"),
        whole_hash: Default::default(),
        decl_name: Arc::from("Record"),
    };
    let lit_ui = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "ui".to_string(),
    )));
    let lit_button = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "button".to_string(),
    )));
    let any_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let inner_record = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: builtin_record_identity(),
        args: Arc::from(vec![lit_button, any_node].into_boxed_slice()),
    });
    let target = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: builtin_record_identity(),
        args: Arc::from(vec![lit_ui, inner_record].into_boxed_slice()),
    });

    let guard = CaptureToken::start_for_query("relation_oracle_transit_probe");
    let (verdict, _fence) = dispatch.relate_nodes(app_config, target);
    let snapshot = guard.end();

    // Firing proof: Record-target recognition holds under the transit
    // demand — the oracle decided the check instead of deferring.
    assert!(
        matches!(verdict, RelationResult::Assignable { .. }),
        "AppConfig must relate Assignable to Record<'ui', Record<'button', any>> \
         through the Object-vs-Record oracle arm, got {verdict:?}"
    );

    let published: Vec<String> = snapshot
        .dispatch_log
        .iter()
        .filter(|e| key_is_published_any_mode(&e.key))
        .map(|e| format!("{:?}", e.key))
        .collect();
    assert!(
        published.is_empty(),
        "the relation-oracle route recorded {} `Published(*)`-context semantic-query key(s); \
         the oracle is an internal transit consumer — every evaluation it demands is keyed \
         under the StructuralTransit demand identity. Offending keys:\n{}",
        published.len(),
        published.join("\n")
    );
}

// ===========================================================================
// #7 — genuine-Expanded conditional distribution (direct dispatch pair)
// ===========================================================================

const SEL_TYPES_TS: &str = r#"import type { Opts, TableState } from 'ttable';
export interface EditorEmits {
  (e: 'save', v: string): void;
}
export interface ViewerEmits {
  (e: 'view', v: number): void;
}
export type Sel<Mode> = Mode extends 'editor' ? EditorEmits : ViewerEmits;
export type NestedCond<T> = { [K in 'a' | 'b']: T extends { meta: infer M } ? Opts<M> : State<T> };
"#;

/// Instantiate `decl_name` from `/workspace/src/sel.ts` with EMPTY
/// args under Skeleton demand (unbound type parameters become
/// `TypeParam` shells — the genuine-Expanded consumer's entry shape
/// for an open generic), returning the instantiated body node.
fn skeleton_instantiate(
    host: &Arc<VerterHost>,
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    decl_name: &str,
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::{
        InstantiateContext, QueryResult, SemanticQueryApi, SemanticQueryOutput,
    };
    host.shallow_file_state("/workspace/src/sel.ts")
        .expect("sel.ts must have shallow file state");
    let key = SemanticQueryKey::Instantiate {
        base: dispatch.type_slot_for(Arc::from("/workspace/src/sel.ts"), Arc::from(decl_name)),
        args: Arc::from(Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice()),
        context: InstantiateContext::non_file_for_tests(
            ProjectionReductionContext::published(ProjectionMode::Skeleton),
            Default::default(),
        ),
    };
    match dispatch.execute_type_node(key) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("skeleton instantiate of {decl_name} must produce a node, got {other:?}"),
    }
}

/// Guard: a QUERY-ROOT unbound conditional under genuine `Expanded`
/// demand still distributes — both branches surface (the
/// inherited-emits seed `defineEmits<Mode extends 'editor' ?
/// EditorEmits : ViewerEmits>` contract). The seed graph shape is an
/// unbound-`TypeParam` check with declaration-anchor branches (the
/// shape eager Expanded payload lowering produces for the emits
/// seed); the empty-path expander instantiates each branch anchor
/// and surfaces both expanded bodies as a Union.
#[test]
fn root_conditional_still_distributes() {
    use crate::semantic_query::{
        QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticQueryApi,
        SemanticQueryOutput,
    };

    let host = build_host(&[
        (
            "/workspace/node_modules/ttable/package.json",
            r#"{ "name": "ttable", "types": "./index.d.ts" }"#,
        ),
        ("/workspace/node_modules/ttable/index.d.ts", WEB_TYPES_TS),
        ("/workspace/src/sel.ts", SEL_TYPES_TS),
    ]);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&*host);
    // The Skeleton instantiation supplies the unbound-`TypeParam`
    // check / extends pair; the branch anchors come from ResolveDecl
    // (the declaration-anchor shape the seed's eager payload lowering
    // hands the expander).
    let skeleton_cond = skeleton_instantiate(&host, &dispatch, "Sel");
    let graph = host.project_type_store().semantic_graph();
    let (check, extends) = match graph
        .node_data(skeleton_cond)
        .as_deref()
        .expect("skeleton conditional node exists")
    {
        SemanticNodeData::Conditional { check, extends, .. } => (*check, *extends),
        other => panic!("Sel skeleton body must be a Conditional shell, got {other:?}"),
    };
    let branch_anchor = |name: &str| -> crate::semantic_query::SemanticNodeId {
        match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::from("/workspace/src/sel.ts"),
                local_scope: None,
            },
            name: Arc::from(name),
        })) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
            other => panic!("ResolveDecl({name}) must anchor, got {other:?}"),
        }
    };
    let true_anchor = branch_anchor("EditorEmits");
    let false_anchor = branch_anchor("ViewerEmits");
    let cond_node = graph.intern_node(SemanticNodeData::Conditional {
        check,
        extends,
        true_branch_ref: true_anchor,
        false_branch_ref: false_anchor,
        distributive: false,
    });

    let read = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: cond_node,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    });
    let result = match read {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("root open-conditional Expanded projection must succeed, got {other:?}"),
    };
    let data = graph
        .node_data(result)
        .expect("distribution result node exists");
    let arms = match data.as_ref() {
        SemanticNodeData::Union(arms) if arms.len() == 2 => arms.clone(),
        other => panic!(
            "a query-root unbound conditional under Expanded must distribute into a \
             two-arm Union (both branches surface), got {other:?}"
        ),
    };
    // Both branches expanded past their declaration anchors: each arm
    // is an interpretable surface, not the bare anchor it started as.
    for (arm, anchor) in arms.iter().zip([true_anchor, false_anchor]) {
        assert_ne!(
            *arm, anchor,
            "distributed branch must expand past its declaration anchor"
        );
    }
}

/// Deferred-arm tracker (`genuine-expanded-nested-conditional-carrier`
/// follow-up): an open conditional in NESTED position (a mapped
/// VALUE) under genuine `Expanded` demand must preserve the deferred
/// carrier instead of distributing per key per branch.
///
/// Deferral evidence (the D2 gate): no FAIL-pre fixture exists — the
/// Expanded empty-path expander walks top-level surface composition
/// only (it has no member-value descent and deliberately no
/// `DeclRef` / `InstantiationRef` arms), and the mapped family's
/// open-value carrier-stop already owns the mapped-value entrance at
/// every route and mode, so no shipped surface reaches an open
/// conditional in a nested position under Expanded today. The
/// nested-position discriminant arm therefore does NOT land
/// speculatively; this tracker pins the required carrier behaviour
/// for the follow-up that exposes such a route.
#[test]
#[ignore = "FOLLOWUP (genuine-expanded-nested-conditional-carrier): no shipped surface \
            reaches an open conditional in nested position under Expanded — the empty-path \
            expander walks top-level composition only and the open-mapped carrier-stop owns \
            the mapped-value entrance; un-ignore when a genuine-Expanded nested route lands"]
fn nested_open_conditional_not_distributed_under_expanded() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::semantic_query::{QueryResult, SemanticQueryApi, SemanticQueryOutput};

    let host = build_host(&[
        (
            "/workspace/node_modules/ttable/package.json",
            r#"{ "name": "ttable", "types": "./index.d.ts" }"#,
        ),
        ("/workspace/node_modules/ttable/index.d.ts", WEB_TYPES_TS),
        ("/workspace/src/sel.ts", SEL_TYPES_TS),
    ]);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&*host);
    let mapped_node = skeleton_instantiate(&host, &dispatch, "NestedCond");

    // Tight projection budget on the direct-dispatch request: a
    // per-key both-branch distribution through the compounding web
    // trips it; carrier preservation never approaches it.
    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        host.next_request_id(),
        Arc::from("/workspace/src/sel.ts"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        TIGHT_PROJECTION_BUDGET,
    );
    let _guard = RequestContextGuard::install(ctx);

    let read = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: mapped_node,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    });
    let result = match read {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!(
            "nested open-conditional Expanded projection must terminate with a \
             carrier-preserving value, got {other:?}"
        ),
    };
    let graph = host.project_type_store().semantic_graph();
    let data = graph.node_data(result).expect("result node exists");
    // The open-valued mapped surface stays a deferred carrier: the
    // per-key values must NOT distribute into both-branch unions
    // materialised through the web.
    let rendered = format!("{:?}", data.as_ref());
    assert!(
        !rendered.contains("BudgetExceeded"),
        "nested open-conditional projection must not burn the projection budget \
         distributing branches, got {rendered}"
    );
}

// ===========================================================================
// #8 — silent-miss probe stays carrier-honest
// ===========================================================================

const OPEN_MAPPED_SLOTS_SFC: &str = r#"<script setup lang="ts" generic="T">
defineSlots<{ [K in keyof T & string]?: (p: { v: T[K] }) => string[] }>();
</script>
<template><div /></template>
"#;

/// The import target EXISTS but does not export the requested name:
/// the payload lowers to a `DeclRef` carrier (the import mapping is a
/// shallow fact), the shallow surface walker fails declaration
/// resolution and contributes an EMPTY surface, and the silent-miss
/// compensation probe must classify it as unresolved.
const MISSING_TS: &str = r#"export const unrelated = 1;
"#;

const UNRESOLVED_SLOTS_SFC: &str = r#"<script setup lang="ts">
import type { NopeSlots } from './missing';
defineSlots<NopeSlots>();
</script>
<template><div /></template>
"#;

/// The empty-payload compensation probe distinguishes
/// carrier-stopped-empty from unresolved-empty STRUCTURALLY, without
/// Expanded-lowering the full macro type argument:
///
/// - an OPEN mapped slots payload (carrier-stopped, legitimately
///   empty surface) triggers NO `Published(Expanded)` projection
///   context and NO unresolved-decl diagnostic;
/// - a genuinely-unresolved import still produces the
///   `macro-payload-decl-unresolved` diagnostic.
#[test]
fn silent_miss_probe_does_not_expand_carrier_stopped_payloads() {
    // (a) open-mapped payload: carrier-stopped empty, no Expanded probe.
    let host = build_host(&[("/workspace/src/OpenSlots.vue", OPEN_MAPPED_SLOTS_SFC)]);
    let guard = CaptureToken::start_for_query("silent_miss_probe_open_mapped");
    let resolved = host.get_component_meta_with_resolution("/workspace/src/OpenSlots.vue");
    let snapshot = guard.end();
    let (meta, _) = resolved.expect("open-mapped slots SFC must resolve");
    let expanded = published_expanded_dispatches(&snapshot.dispatch_log);
    assert!(
        expanded.is_empty(),
        "the silent-miss probe must NOT Expanded-lower a carrier-stopped payload; \
         recorded:\n{}",
        expanded.join("\n")
    );
    assert!(
        !meta.macro_expansion_diagnostics.iter().any(|d| {
            d.diagnostics
                .iter()
                .any(|diag| diag.context.contains("macro-payload-decl-unresolved"))
        }),
        "a carrier-stopped open-mapped payload is NOT an unresolved declaration"
    );

    // (b) genuinely-unresolved import keeps its diagnostic.
    let host = build_host(&[
        ("/workspace/src/missing.ts", MISSING_TS),
        ("/workspace/src/Broken.vue", UNRESOLVED_SLOTS_SFC),
    ]);
    let resolved = host.get_component_meta_with_resolution("/workspace/src/Broken.vue");
    let (meta, _) = resolved.expect("unresolved-import SFC must still produce meta");
    assert!(
        meta.macro_expansion_diagnostics.iter().any(|d| {
            d.diagnostics
                .iter()
                .any(|diag| diag.context.contains("macro-payload-decl-unresolved"))
        }),
        "a genuinely-unresolved macro payload import must surface the \
         `macro-payload-decl-unresolved` diagnostic; got {:?}",
        meta.macro_expansion_diagnostics
    );
}

/// An owner-local `interface Extended extends Base { ... }` consumed
/// through a prop lowers to the heritage intersection
/// `Intersection([Ref{Base}, Object{own members}])` in the analyzer
/// body. The registry sidecar entry the consumer re-resolves through
/// MUST publish the heritage-MERGED one-level surface — a single
/// `Object` carrying base + own members with SHALLOW member values —
/// composed through the shared empty-path Shallow surface walker, not
/// the raw un-merged intersection (which a registry consumer cannot
/// interpret as a member surface).
#[test]
fn interface_extends_registry_publishes_heritage_merged_shallow_surface() {
    use verter_type_expr::ObjectMember;

    let host = build_host(&[(
        "/workspace/src/IE.vue",
        r#"<script setup lang="ts">
interface Base {
  id: number;
  name: string;
}
interface Extended extends Base {
  email: string;
  active?: boolean;
}
defineProps<{ user: Extended }>();
</script>
<template><div /></template>
"#,
    )]);
    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/IE.vue")
        .expect("InterfaceExtends SFC must resolve");

    // The published prop stays the shallow alias carrier (Shallow-By-
    // Default): the consumer re-resolves `Extended` through the registry.
    let user = meta
        .props
        .iter()
        .find(|p| p.name == "user")
        .expect("`user` prop published");
    assert!(
        matches!(&user.type_expr, TypeExpr::Ref { name, .. } if name.as_ref() == "Extended"),
        "`user` publishes the shallow `Ref {{ Extended }}` carrier; got {:?}",
        user.type_expr
    );

    let entry = resolution
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Extended")
        .expect("registry publishes the `Extended` sidecar entry");
    let TypeExpr::Object(object) = &entry.type_expr else {
        panic!(
            "`Extended` registry entry must be the heritage-merged Object \
             surface, not the raw heritage intersection; got {:?}",
            entry.type_expr
        );
    };
    let mut keys: Vec<&str> = Vec::new();
    for member in &object.properties {
        let ObjectMember::Property(property) = member else {
            panic!("unexpected non-property member: {member:?}");
        };
        keys.push(property.name.as_str());
        // One-level surface, member values SHALLOW: every value here is a
        // primitive — none may have been replaced by a nested expansion,
        // an intersection arm, or an unresolved sentinel.
        let (expected_primitive, expected_optional) = match property.name.as_str() {
            "id" => (verter_type_expr::PrimitiveName::Number, false),
            "name" => (verter_type_expr::PrimitiveName::String, false),
            "email" => (verter_type_expr::PrimitiveName::String, false),
            "active" => (verter_type_expr::PrimitiveName::Boolean, true),
            other => panic!("unexpected merged member `{other}`"),
        };
        assert!(
            matches!(&property.ty, TypeExpr::Primitive(p) if *p == expected_primitive),
            "member `{}` keeps its shallow primitive value; got {:?}",
            property.name,
            property.ty
        );
        assert_eq!(
            property.optional, expected_optional,
            "member `{}` optionality must survive the heritage merge",
            property.name
        );
    }
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["active", "email", "id", "name"],
        "heritage merge surfaces base + own members exactly once each"
    );
}

// ===========================================================================
// typeof value-graph demand — the TypeOf query lowers at the requested mode
// ===========================================================================

/// The web package's `index.d.ts` extended with a VALUE declaration
/// typed against the compounding generic web — the `typeof` lane's
/// value graph. A demand-blind `TypeOf` build lowers this annotation
/// eagerly at `Expanded`, detonating the same instantiation storm the
/// carrier-preserving decl-body lowering rule exists to prevent.
fn web_types_with_value_dts() -> String {
    format!("{WEB_TYPES_TS}export declare const coreFactory: CoreOptions<RowData>;\n")
}

/// Workspace helper whose BODY crosses a `typeof`-typed value — the
/// shape the ref-cycle BFS's Skeleton-mode body lowering walks.
const TYPEOF_HELPER_TS: &str = r#"import { coreFactory } from 'ttable';
export type FactoryBag<T> = { factory: typeof coreFactory; item: T };
"#;

/// The corpus shape: a NAMED props interface (Ref carrier — the field
/// eval's fast path) whose member value crosses the `typeof`-typed
/// value. The publication reduce then owns the field at `Navigate`.
/// An anonymous inline-literal payload would instead enter the
/// compound-carrier field-eval slow path, which lowers the carrier at
/// `Expanded` by request-mode design — a separate, typeof-independent
/// axis outside this contract.
const TYPEOF_SFC_VUE: &str = r#"<script setup lang="ts">
import { coreFactory } from 'ttable';
interface Props {
  factory?: typeof coreFactory;
  label?: string;
}
defineProps<Props>();
</script>
<template><div /></template>
"#;

/// A `Skeleton`-demand instantiation whose body lowering crosses a
/// `typeof`-typed value annotation lowers that value's declaration
/// graph AT THE REQUESTED DEMAND: the `TypeOf` query carries the
/// caller's projection-reduction context, so the value annotation
/// lowers carrier-preserving (`CoreOptions<RowData>` interns as an
/// instantiation-shaped carrier) instead of executing an Expanded
/// materialisation of the transitive value web at build time.
///
/// A demand-blind `TypeOf` build hard-codes `Expanded` into its
/// lowering calls, records compounding `Instantiate` dispatches with
/// `Published(Expanded)` contexts on the stream, and trips the tight
/// projection budget.
#[test]
fn typeof_value_graph_lowers_at_requested_demand() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::semantic_query::{
        InstantiateContext, QueryResult, SemanticQueryApi, SemanticQueryOutput,
    };

    let web_dts = web_types_with_value_dts();
    let host = build_host(&[
        (
            "/workspace/node_modules/ttable/package.json",
            r#"{ "name": "ttable", "types": "./index.d.ts" }"#,
        ),
        (
            "/workspace/node_modules/ttable/index.d.ts",
            web_dts.as_str(),
        ),
        ("/workspace/src/factory.ts", TYPEOF_HELPER_TS),
    ]);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&*host);
    host.shallow_file_state("/workspace/src/factory.ts")
        .expect("factory.ts must have shallow file state");

    // Tight projection budget on the direct-dispatch request: an
    // Expanded lowering of the value web trips it; demand-faithful
    // Skeleton lowering never approaches it.
    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        host.next_request_id(),
        Arc::from("/workspace/src/factory.ts"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        TIGHT_PROJECTION_BUDGET,
    );
    let _guard = RequestContextGuard::install(ctx);

    let guard = CaptureToken::start_for_query("typeof_demand_skeleton");
    let read = dispatch.execute_type_node(SemanticQueryKey::Instantiate {
        base: dispatch.type_slot_for(
            Arc::from("/workspace/src/factory.ts"),
            Arc::from("FactoryBag"),
        ),
        args: Arc::from(Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice()),
        context: InstantiateContext::non_file_for_tests(
            ProjectionReductionContext::published(ProjectionMode::Skeleton),
            Default::default(),
        ),
    });
    let snapshot = guard.end();

    let expanded = published_expanded_dispatches(&snapshot.dispatch_log);
    assert!(
        expanded.is_empty(),
        "a Skeleton-demand body lowering crossing `typeof coreFactory` recorded {} \
         `Published(Expanded)` projection context(s) — the `TypeOf` build must lower \
         the value graph at the REQUESTED demand, never a hard-coded Expanded. \
         Offending keys:\n{}",
        expanded.len(),
        expanded.join("\n")
    );
    match read {
        QueryResult::Value(SemanticQueryOutput { .. }) => {}
        other => panic!("Skeleton instantiate of FactoryBag must produce a node, got {other:?}"),
    }
    let cold = cold_instantiate_dispatches(&snapshot.dispatch_log);
    assert!(
        cold < 40,
        "cold `Instantiate` dispatches must stay bounded under Skeleton demand \
         (got {cold}; an Expanded value-graph lowering compounds the web)"
    );
}

/// A publication request whose macro payload crosses a `typeof`-typed
/// value resolves WITHOUT a budget trip and records ZERO
/// `Published(Expanded)` projection contexts: the payload's ambient
/// lowering demand rides the `TypeOf` query, so the value annotation
/// publishes as a shallow carrier instead of materialising the
/// transitive value web.
#[test]
fn typeof_macro_payload_publication_stays_bounded() {
    let web_dts = web_types_with_value_dts();
    let host = build_host_with_budget(
        &[
            (
                "/workspace/node_modules/ttable/package.json",
                r#"{ "name": "ttable", "types": "./index.d.ts" }"#,
            ),
            (
                "/workspace/node_modules/ttable/index.d.ts",
                web_dts.as_str(),
            ),
            ("/workspace/src/Factory.vue", TYPEOF_SFC_VUE),
        ],
        TIGHT_PROJECTION_BUDGET,
    );

    let guard = CaptureToken::start_for_query("typeof_demand_publication");
    let resolved = host.get_component_meta_with_resolution("/workspace/src/Factory.vue");
    let snapshot = guard.end();
    let (meta, resolution) = resolved.expect("typeof-prop SFC must resolve");

    assert!(
        !resolution.synthesis_should_suppress,
        "publication crossing `typeof coreFactory` must complete WITHOUT a budget \
         trip (synthesis_should_suppress=true means the demand-blind TypeOf build \
         Expanded-lowered the value web and tripped the \
         {TIGHT_PROJECTION_BUDGET}-op fuse)"
    );
    let expanded = published_expanded_dispatches(&snapshot.dispatch_log);
    assert!(
        expanded.is_empty(),
        "publication recorded {} `Published(Expanded)` projection context(s) through \
         the `typeof` lane; publication demand is Navigate-only. Offending keys:\n{}",
        expanded.len(),
        expanded.join("\n")
    );
    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    for key in ["factory", "label"] {
        assert!(
            names.contains(&key),
            "prop `{key}` must publish (got {names:?}) — demand-faithful typeof \
             lowering must NOT drop the typeof-typed member"
        );
    }
    let cold = cold_instantiate_dispatches(&snapshot.dispatch_log);
    assert!(
        cold < 40,
        "cold `Instantiate` dispatches must stay bounded (got {cold}; an Expanded \
         value-graph lowering compounds the web)"
    );
}

// ===========================================================================
// completeness honesty — admission refusal is NOT a partial result
// ===========================================================================

/// A COMPLETE slot-binding synthesis whose nested structural
/// materialisation is refused warm-cache ADMISSION (post-compute
/// revalidation rejects the freshly-built entry — e.g. the request
/// view's fact snapshot predates the lazily-parsed helper file) must
/// NOT publish `synthesis_should_suppress`: the computed child surface
/// is complete by construction (a genuine in-scope partial routes
/// through `ReturnOnly` with the partial bit BEFORE admission), so
/// refusing admission is benign non-cacheability, never a partial.
///
/// The slot bindings themselves must still publish — the suppress flag
/// and the published surface must AGREE that resolution is complete.
#[test]
fn slot_binding_synthesis_admission_refusal_is_not_partial() {
    let host = build_host(&[
        (
            "/workspace/src/icon-types.ts",
            "export interface IconProps {\n  name: string;\n  size?: number;\n}\n",
        ),
        (
            "/workspace/src/child-types.ts",
            "import type { IconProps } from './icon-types';\n\
             export interface ChildSlots {\n  default?(props: { icon: IconProps }): any;\n}\n",
        ),
        (
            "/workspace/src/App.vue",
            "<script setup lang=\"ts\">\nimport type { ChildSlots } from './child-types'\n\
             defineSlots<ChildSlots>()\n</script>\n<template><div /></template>\n",
        ),
    ]);

    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/App.vue")
        .expect("imported-slots SFC must resolve");

    let slot = meta
        .slots
        .iter()
        .find(|s| s.name == "default")
        .expect("default slot published");
    assert!(
        slot.bindings.iter().any(|b| b.name == "icon"),
        "the `icon` binding must publish (got {:?})",
        slot.bindings.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
    assert!(
        !resolution.synthesis_should_suppress,
        "a complete slot-binding synthesis must NOT suppress: nested materialise \
         admission refusal (benign non-cacheability) was laundered into \
         result_is_partial — the false-partial class"
    );
}
