// The oracle-query-spec registry: the machine-readable, checked-in source of
// truth for the executable query payloads a lifted `TypeExpr`-projection row
// issues (`docs/arch/u0-oracle-harness-design.md` §Q4).
//
// The manifest is a CLOSED ROW LEDGER and carries no executable query payload;
// one row can issue N queries. This registry moves the payloads OFF the test
// bodies and INTO a closed Rust table keyed `(row_file, row_function,
// query_ordinal)`, so query coverage is true BY CONSTRUCTION: a lifted body
// just calls the shared driver over its registry entries.
//
// PURITY CONTRACT (`oracle_query_specs_is_pure_data`). This file is PURE
// context-neutral data — closed enums + owned `&'static str`, plus pure
// self-contained validation fns over its OWN types. It has NO `use super::*`,
// NO reference to the unit-test `support` module, NO private unit-test types,
// and NO helper calls. That is what lets the SAME table be reached two ways
// without drifting into two copies: the lifted unit tests in `src/typeinfo/`
// reach it as the `oracle_query_specs` module, and the `tests/` coverage guard
// reaches it via an `include!` of this exact file — both compile against ONE
// table, and the `tests/` side never needs the unit-test `support` module.
// (Plain `//` comments, not a `//!` module doc, so this file `include!`s
// cleanly into the `tests/` guard — an inner doc comment is illegal in an
// `include!`d position.)
//
// The registry seats the 19 lifted rows (the two index-signature
// publication queries + the two built-in modifier-utility queries + the three
// U2 IndexedAccess-reduction carve-out queries + the mapped-modifier `-?`
// carve-out query at U2.MAPPED_TEMPLATE + the three keyof-expansion carve-out
// queries captured through the distributive-identity scaffold + the eight
// U2.UTILITIES reducer queries: five Awaited rows, two NonNullable rows, and
// the variadic-spread Concat row). The types + validation are additionally
// exercised with synthetic specs by the discriminating guards.

/// The content id of the CURRENT closed vendored oracle-env corpus — the
/// pinned-env constant the registry + every guard read to derive a
/// `snapshot_id` and locate the corpus dir
/// (`oracle_env/<env_corpus_dir_name(env_corpus_id)>/`) WITHOUT opening a
/// snapshot (§Q4). Pinned by the snapshot generator when the corpus is
/// (re-)vendored — `gen::compute_env_corpus_id`, a BLAKE3 domain-separated
/// digest over the canonical-path-sorted corpus listing. The harness guards
/// assert the derived corpus root exists on disk (with its
/// `oracle.tsconfig.json`) and its dir name stays path-portable.
#[allow(dead_code)]
pub(crate) const CURRENT_ENV_CORPUS_ID: &str =
    "blake3:c6c4bda7c5c5106e873a66c7da516f6a0545492e280dfb9e964f833ed0e8d8f7";

/// Stable hash of the EFFECTIVE canonical `oracle.tsconfig.json` vendored in
/// the corpus (§Q2 "Env pinning") — `identity::content_hash` over the
/// canonicalized config content. Pinned by the snapshot generator alongside
/// `CURRENT_ENV_CORPUS_ID` when the corpus is (re-)vendored; enters every
/// `snapshot_id` through `PinnedEnv`.
#[allow(dead_code)]
pub(crate) const COMPILER_OPTIONS_HASH: &str =
    "sha256:f062f067510fb7c74440a69b64c0d8281f24f39a9785997d32352049e2519643";

/// The lookup table the resolver name is resolved IN. Derived from the query
/// (§Q4 `source_locator`), never an independent steering input.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolSpace {
    Type,
    Value,
}

/// The projection mode the helper resolves under, and the mode the row's oracle
/// snapshot is captured + compared in. The spec's `projection_mode` IS the
/// oracle query identity: the driver resolves Verter's projection in this mode
/// and the keystone trace + audit-mode guards assert the live audit record
/// reports this same mode. `Shallow` / `Navigate` / `Expanded` are all in use;
/// `Skeleton` is carried for schema totality (the BFS / generic-helper traversal
/// mode, not yet a snapshot-row mode).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionModeSpec {
    Shallow,
    Navigate,
    Expanded,
    Skeleton,
}

/// The host/project setup kind. `standalone` is the only first-class kind; the
/// others are carried for schema totality (deferred to the env-pin spike).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostSetupKindSpec {
    Standalone,
    WorkspaceFootprint,
    PackageBacked,
}

/// The oracle value kind this entry produces. Only `structured_type_expr` for
/// every entry this harness writes; a future kind bumps `oracle_schema_version`.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OracleValueKindSpec {
    StructuredTypeExpr,
}

/// One file the row upserts into its workspace: the canonical leading-slash
/// path + the SOURCE BYTES the test upserts (the registry is the source-byte
/// authority; the snapshot stores only `{ path, content_hash }`).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkspaceFileSpec {
    pub(crate) path: &'static str,
    pub(crate) source: &'static str,
}

/// The host/project setup axes the row's helper constructed.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostProjectSpec {
    pub(crate) project_root: &'static str,
    pub(crate) workspace_root: &'static str,
    pub(crate) tsconfig_path: &'static str,
    pub(crate) host_setup_kind: HostSetupKindSpec,
}

/// The typed locator the source-side allowlist walk + binding-identity check
/// start from. GUARD-ONLY: not a value-affecting `snapshot_id` input (§Q4) — the
/// `symbol_space` is DERIVED FROM the query, never an independent steering input.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceLocatorSpec {
    pub(crate) reference_canonical: &'static str,
    pub(crate) reference_name: &'static str,
    pub(crate) symbol_space: SymbolSpace,
}

/// The probe-RHS capture strategy the generator synthesizes for the query —
/// the registry's declared capture-strategy axis (§Q2 keyof-expansion
/// scaffold). `Bare` is the default bare-symbol RHS; `DistributiveIdentity`
/// wraps the symbol in the inlined per-query identity helper
/// `type __oracle_probe_dist__N<T> = T extends never ? never : T;` so tsgo
/// prints the EXPANDED member union instead of echoing the written
/// `keyof <operand>` display origin. The generator CROSS-CHECKS the declared
/// strategy against the live source-walk carve-out classification —
/// `DistributiveIdentity` is admissible ONLY for Expanded-mode keyof
/// carve-out rows (`KeyofBareRef` / `KeyofSelfIndex`).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeRhsSpec {
    Bare,
    DistributiveIdentity,
}

/// Which `support.rs` helper produces the in-process `TypeExpr`, with its
/// kind-specific payload (§Q4). The `*Expr` suffix mirrors the design-mandated
/// helper names; the shared suffix is intentional, not a naming smell.
#[allow(dead_code, clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryHelperSpec {
    /// `resolve_expr(host, canonical, symbol, type_args, mode)`. `type_args` is
    /// the canonical `TypeExpr`-JSON of each argument (empty for the common
    /// non-generic case; non-empty rows are deferred until the printer spike).
    /// `probe_rhs` is the declared capture strategy (only `ResolveExpr` can
    /// carry a non-`Bare` one).
    ResolveExpr {
        symbol: &'static str,
        type_args: &'static [&'static str],
        projection_mode: ProjectionModeSpec,
        probe_rhs: ProbeRhsSpec,
    },
    /// `shallow_surface_expr(host, canonical, symbol)` — always empty-path
    /// `Shallow`.
    ShallowSurfaceExpr { symbol: &'static str },
    /// `evaluate_expr(host, scope, expression, mode)` over a type-position-valid
    /// SINGLE-ROOT expression (e.g. `typeof f`); the `source_locator` names the
    /// single root binder.
    EvaluateExpr {
        expression: &'static str,
        projection_mode: ProjectionModeSpec,
    },
}

/// One `(row_file, row_function, query_ordinal)` registry entry — the full
/// executable query spec (§Q4). PURE data: every field is a closed enum or an
/// owned `&'static str` / static slice. `oracle_family` is CARRIED ON THE ENTRY
/// (the directory/presentation key the driver builds the snapshot path from).
/// The entry carries no separate "obligation" set: a row's `assert_query_mode`
/// that matches its own `projection_mode` is oracle query IDENTITY (proven live
/// by the driver running this mode and the audit-mode guard asserting the record
/// reports it), not a side obligation. An INDEPENDENT non-`TypeExpr` assertion
/// (dependency footprint, audit-record specifics, divergence correction) is the
/// only thing that would promote a row to `ProofRequirement::OracleAndGuard`
/// with a registered live prover — none of the four seated rows carry one.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuerySpec {
    pub(crate) row_file: &'static str,
    pub(crate) row_function: &'static str,
    pub(crate) query_ordinal: u16,
    pub(crate) oracle_family: &'static str,
    pub(crate) workspace_files: &'static [WorkspaceFileSpec],
    pub(crate) primary_canonical: &'static str,
    pub(crate) host_project: HostProjectSpec,
    pub(crate) query_helper: QueryHelperSpec,
    pub(crate) source_locator: SourceLocatorSpec,
    pub(crate) oracle_value_kind: OracleValueKindSpec,
}

/// The vendored source bytes of `/fixtures/index_signatures.ts` — the registry
/// is the source-byte authority (the snapshot stores only `{ path, content_hash
/// }`). Inlined verbatim (PURE owned `&'static str`, no `include_str!` helper
/// call) so the `include!`d `tests/` guard compiles against the same bytes.
#[allow(dead_code)]
pub(crate) const INDEX_SIGNATURES_SOURCE: &str = r#"// @ai-generated - Synthetic index-signature typeinfo fixture.

export type NumericIndexed = { [key: number]: string };
export type SymbolIndexed = { [key: symbol]: number };
export type DualIndexed = {
  [key: string]: number | boolean;
  [key: number]: number;
};

// Numeric lookup against a numeric index signature.
export type NumericLookup = NumericIndexed[42];

// Symbol lookup against a symbol index signature.
export type SymbolLookup = SymbolIndexed[symbol];

// String lookup against a dual index signature must return the string-key
// value type union (not the number-key value type).
export type DualStringLookup = DualIndexed["any-string-here"];

// Numeric lookup against a dual index signature must return the
// numeric-key value type (number takes priority when both signatures match).
export type DualNumberLookup = DualIndexed[0];
"#;

/// The workspace-file set the two index-signature publication rows upsert: the
/// single canonical fixture, shared by both rows.
#[allow(dead_code)]
const INDEX_SIGNATURES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/index_signatures.ts",
    source: INDEX_SIGNATURES_SOURCE,
}];

/// One `ResolveExpr`/`Expanded` query spec for an index-signature publication
/// row (`NumericIndexed` / `SymbolIndexed`). Both resolve a declared object-type
/// alias and publish its terminal index-signature surface — the foundational
/// decl-resolution publication, NOT indexed-access reduction. Verified in the
/// row's original `Expanded` projection mode: tsgo expands the alias and Verter's
/// `Expanded` projection produces the same terminal index-signature surface.
const fn index_signature_publication_spec(
    row_function: &'static str,
    symbol: &'static str,
) -> QuerySpec {
    QuerySpec {
        row_file: "index_signatures.rs",
        row_function,
        query_ordinal: 0,
        oracle_family: "index_signatures",
        workspace_files: INDEX_SIGNATURES_FILES,
        primary_canonical: "/fixtures/index_signatures.ts",
        host_project: HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol,
            type_args: &[],
            projection_mode: ProjectionModeSpec::Expanded,
            probe_rhs: ProbeRhsSpec::Bare,
        },
        source_locator: SourceLocatorSpec {
            reference_canonical: "/fixtures/index_signatures.ts",
            reference_name: symbol,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// The vendored source bytes of `/fixtures/utility_edge.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`) so the
/// `include!`d `tests/` guard compiles against the same bytes. MUST be
/// byte-identical to `fixtures/utility_edge.ts`.
#[allow(dead_code)]
pub(crate) const UTILITY_EDGE_SOURCE: &str = r#"// @ai-generated - Synthetic utility-edge typeinfo fixture.

export type Base = { a: number; b: string; c: boolean };

export type PickNever = Pick<Base, never>;
export type OmitNever = Omit<Base, never>;
export type OmitAll = Omit<Base, keyof Base>;
export type PickAll = Pick<Base, keyof Base>;

export type Optional = { a?: string; b?: number };
export type RequiredOptional = Required<Optional>;
export type ReadonlyRequiredOptional = Readonly<Required<Optional>>;

export type Nullable = string | null | undefined;
export type NonNullablePrim = NonNullable<Nullable>;
export type ExtractStringOnly = Extract<string | number | boolean, string>;
export type ExcludeNumberOnly = Exclude<string | number | boolean, number>;
"#;

/// The workspace-file set the two built-in modifier-utility rows upsert: the
/// single canonical fixture, shared by both rows.
#[allow(dead_code)]
const UTILITY_EDGE_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/utility_edge.ts",
    source: UTILITY_EDGE_SOURCE,
}];

/// One `ResolveExpr`/`Expanded` query spec for a built-in modifier-utility row
/// (`RequiredOptional` / `ReadonlyRequiredOptional`). Both resolve a declared
/// library mapped-type alias and publish its remapped object surface — the
/// terminal mapped-template `-?`/`readonly` remap, NOT indexed-access reduction.
/// Verified in the row's original `Expanded` projection mode: tsgo expands the
/// utility alias and Verter's `Expanded` projection produces the same remapped
/// object surface.
const fn utility_edge_modifier_spec(row_function: &'static str, symbol: &'static str) -> QuerySpec {
    QuerySpec {
        row_file: "utility_edge.rs",
        row_function,
        query_ordinal: 0,
        oracle_family: "utility_edge",
        workspace_files: UTILITY_EDGE_FILES,
        primary_canonical: "/fixtures/utility_edge.ts",
        host_project: HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol,
            type_args: &[],
            projection_mode: ProjectionModeSpec::Expanded,
            probe_rhs: ProbeRhsSpec::Bare,
        },
        source_locator: SourceLocatorSpec {
            reference_canonical: "/fixtures/utility_edge.ts",
            reference_name: symbol,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// The vendored source bytes of `/fixtures/typescript-rules.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `oracle_query_specs_guard` asserts byte-identity with `fixtures/typescript_rules.ts`.
#[allow(dead_code)]
pub(crate) const TYPESCRIPT_RULES_SOURCE: &str = r#"// @ai-generated - Synthetic TypeScript type-system rules fixture.

export type LiteralAndPrimitiveSurface = {
  stringLiteral: "ready";
  numberLiteral: 42;
  booleanLiteral: true;
  stringValue: string;
  numberValue: number;
  booleanValue: boolean;
  symbolValue: symbol;
  bigintValue: bigint;
  nullValue: null;
  undefinedValue: undefined;
  unknownValue: unknown;
  anyValue: any;
  neverValue: never;
};

export type MethodAndIndexSurface = {
  readonly id: string;
  label?: string;
  method?: (input: string, count?: number) => boolean;
  [key: string]:
    | string
    | number
    | boolean
    | undefined
    | ((input: string, count?: number) => boolean);
};

export type TupleRules = [name: string, count?: number, ...flags: boolean[]];

export type ReadonlyTupleRules = readonly [mode: "view", values: readonly number[]];

export type FunctionRules = (
  item: { id: string },
  ...flags: boolean[]
) => { id: string; flags: boolean[] };

export type RecordLiteralKeys = Record<"alpha" | "beta", number>;

export type MappedModifierRules<T> = {
  readonly [K in keyof T]-?: T[K];
};

export type MappedModifierSurface = MappedModifierRules<{
  id?: string;
  count?: number;
}>;

export type UnionObjectRules =
  | { kind: "a"; a: string; shared: boolean }
  | { kind: "b"; b: number; shared: boolean };

export type IntersectionObjectRules = { id: string } & { count?: number } & {
  readonly ready: boolean;
};

export interface KeySource {
  id: string;
  count?: number;
  nested: {
    value: string;
  };
}

export type KeyOfRules = keyof KeySource;
export type IndexedRules = KeySource["nested"]["value"];

export type ConditionalDistributive<T> = T extends string ? { text: T } : { other: T };
export type ConditionalDistributedRules = ConditionalDistributive<"a" | 1>;

export type ConditionalNonDistributive<T> = [T] extends [string] ? { text: T } : { other: T };
export type ConditionalNonDistributedRules = ConditionalNonDistributive<"a" | 1>;

export type ConstructorLike = new (id: string) => { id: string; ready: boolean };
export type ConstructorParamsRules = ConstructorParameters<ConstructorLike>;
export type InstanceRules = InstanceType<ConstructorLike>;

export class ClassRules {
  id: string;
  constructor(id: string);
  method(count: number): string;
}
export type ClassInstanceRules = InstanceType<typeof ClassRules>;
export type ClassConstructorParamsRules = ConstructorParameters<typeof ClassRules>;

export const literalConfig = {
  mode: "view",
  nested: {
    value: 1,
  },
} as const;
export type TypeOfConstRules = typeof literalConfig;
export type TypeOfConstNestedValue = typeof literalConfig.nested.value;

export type AwaitedRules = Awaited<Promise<Promise<{ done: true }>>>;

export type TemplateIntrinsicRules = `on${Capitalize<"submit" | "cancel">}`;

export type KeyRemapExcludeRules<T> = {
  [K in keyof T as K extends "internal" ? never : `public:${K & string}`]: T[K];
};
export type KeyRemapExcludeSurface = KeyRemapExcludeRules<{
  id: string;
  internal: boolean;
  count: number;
}>;
"#;

/// The vendored source bytes of `/fixtures/deep-path.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `oracle_query_specs_guard` asserts byte-identity with `fixtures/deep_path.ts`.
#[allow(dead_code)]
pub(crate) const DEEP_PATH_SOURCE: &str = r#"// @ai-generated - Synthetic deep indexed-access typeinfo fixture.

export type TerminalPayload = {
  id: string;
  priority: 1 | 2 | 3;
};

export type HeavySibling00 = {
  ignored00: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling01 = {
  ignored01: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling02 = {
  ignored02: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling03 = {
  ignored03: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling04 = {
  ignored04: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling05 = {
  ignored05: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling06 = {
  ignored06: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling07 = {
  ignored07: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling08 = {
  ignored08: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling09 = {
  ignored09: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling10 = {
  ignored10: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling11 = {
  ignored11: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling12 = {
  ignored12: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling13 = {
  ignored13: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling14 = {
  ignored14: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling15 = {
  ignored15: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type Layer00<T> = { target: T; sibling00?: HeavySibling00 };
export type Layer01<T> = { level00: Layer00<T>; sibling01?: HeavySibling01 };
export type Layer02<T> = { level01: Layer01<T>; sibling02?: HeavySibling02 };
export type Layer03<T> = { level02: Layer02<T>; sibling03?: HeavySibling03 };
export type Layer04<T> = { level03: Layer03<T>; sibling04?: HeavySibling04 };
export type Layer05<T> = { level04: Layer04<T>; sibling05?: HeavySibling05 };
export type Layer06<T> = { level05: Layer05<T>; sibling06?: HeavySibling06 };
export type Layer07<T> = { level06: Layer06<T>; sibling07?: HeavySibling07 };
export type Layer08<T> = { level07: Layer07<T>; sibling08?: HeavySibling08 };
export type Layer09<T> = { level08: Layer08<T>; sibling09?: HeavySibling09 };
export type Layer10<T> = { level09: Layer09<T>; sibling10?: HeavySibling10 };
export type Layer11<T> = { level10: Layer10<T>; sibling11?: HeavySibling11 };
export type Layer12<T> = { level11: Layer11<T>; sibling12?: HeavySibling12 };
export type Layer13<T> = { level12: Layer12<T>; sibling13?: HeavySibling13 };
export type Layer14<T> = { level13: Layer13<T>; sibling14?: HeavySibling14 };
export type Layer15<T> = { level14: Layer14<T>; sibling15?: HeavySibling15 };
export type DeepRoot = Layer15<TerminalPayload>;
export type DeepProjectedTarget =
  DeepRoot["level14"]["level13"]["level12"]["level11"]["level10"]["level09"]["level08"]["level07"]["level06"]["level05"]["level04"]["level03"]["level02"]["level01"]["level00"]["target"];
"#;

/// The vendored source bytes of `/fixtures/wide-deep.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `oracle_query_specs_guard` asserts byte-identity with `fixtures/wide_deep.ts`.
#[allow(dead_code)]
pub(crate) const WIDE_DEEP_SOURCE: &str = r#"// @ai-generated - Synthetic wide/deep component-like typeinfo fixture.

export type Token = "alpha" | "beta" | "gamma";

export interface Leaf {
  id: string;
  score: number;
  flags?: Partial<Record<"pinned" | "active", boolean>>;
}

export type Action = {
  id: string;
  label: string;
  disabled?: boolean;
};

export type WidePanel<TLeaf extends Leaf = Leaf> = {
  header: {
    title: string;
    actions?: Action[];
  };
  row00?: TLeaf;
  row01?: TLeaf;
  row02?: TLeaf;
  row03?: TLeaf;
  row04?: TLeaf;
  row05?: TLeaf;
  row06?: TLeaf;
  row07?: TLeaf;
  row08?: TLeaf;
  row09?: TLeaf;
  row10?: TLeaf;
  row11?: TLeaf;
  row12?: TLeaf;
  row13?: TLeaf;
  row14?: TLeaf;
  row15?: TLeaf;
  nested: {
    level1: {
      level2: {
        target: Pick<TLeaf, "id" | "score"> & {
          token: Token;
        };
      };
    };
  };
};

export type WideDeepSurface = WidePanel;
export type WideDeepProjectedTarget = WidePanel["nested"]["level1"]["level2"]["target"];
export type WideDeepProjectedToken = WidePanel["nested"]["level1"]["level2"]["target"]["token"];
export type WideDeepRowFlags = NonNullable<WidePanel["row00"]>["flags"];
export type WideDeepFlagActive = NonNullable<NonNullable<WidePanel["row00"]>["flags"]>["active"];
"#;

/// The workspace-file set the two `typescript_rules.rs` carve-out rows upsert.
#[allow(dead_code)]
const TYPESCRIPT_RULES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/typescript-rules.ts",
    source: TYPESCRIPT_RULES_SOURCE,
}];

/// The workspace-file set the `deep_path.rs` carve-out row upserts.
#[allow(dead_code)]
const DEEP_PATH_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/deep-path.ts",
    source: DEEP_PATH_SOURCE,
}];

/// The workspace-file set the `wide_deep.rs` carve-out row upserts.
#[allow(dead_code)]
const WIDE_DEEP_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/wide-deep.ts",
    source: WIDE_DEEP_SOURCE,
}];

/// One `ResolveExpr`/`Expanded` query spec for a U2 IndexedAccess-reduction
/// carve-out row whose source body is a `keyof Root` / `Root["a"]["b"]…` operator
/// alias. The shared resolver reduces the operator root to its terminal
/// (string-literal union / named-member projection); tsgo expands the same alias.
const fn carve_out_spec(
    row_file: &'static str,
    row_function: &'static str,
    oracle_family: &'static str,
    workspace_files: &'static [WorkspaceFileSpec],
    primary_canonical: &'static str,
    symbol: &'static str,
) -> QuerySpec {
    QuerySpec {
        row_file,
        row_function,
        query_ordinal: 0,
        oracle_family,
        workspace_files,
        primary_canonical,
        host_project: HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol,
            type_args: &[],
            projection_mode: ProjectionModeSpec::Expanded,
            probe_rhs: ProbeRhsSpec::Bare,
        },
        source_locator: SourceLocatorSpec {
            reference_canonical: primary_canonical,
            reference_name: symbol,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// Vendored source bytes of `/fixtures/mapped_modifiers.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/mapped_modifiers.ts`.
#[allow(dead_code)]
pub(crate) const MAPPED_MODIFIERS_SOURCE: &str = r#"// @ai-generated - Synthetic mapped-type modifier typeinfo fixture.
//
// Exercises the `+readonly` / `-readonly` adders/removers, the `+?` / `-?`
// optional modifier toggles, an `as never` key filter, and a mapped type that
// uses a conditional value expression. All shapes are synthetic — no
// dependency on real libraries.

// ---------------------------------------------------------------------------
// (1) +readonly adder
// ---------------------------------------------------------------------------
export type MutableSource = { a: string; b: number };
export type AllReadonly<T> = { +readonly [K in keyof T]: T[K] };
export type AddReadonlyResult = AllReadonly<MutableSource>;

// ---------------------------------------------------------------------------
// (2) -readonly remover
// ---------------------------------------------------------------------------
export type ReadonlySource = { readonly a: string; readonly b: number };
export type Mutable<T> = { -readonly [K in keyof T]: T[K] };
export type MutableResult = Mutable<ReadonlySource>;

// ---------------------------------------------------------------------------
// (3) +? optional adder
// ---------------------------------------------------------------------------
export type RequiredSource = { a: string; b: number };
export type AllOptional<T> = { [K in keyof T]+?: T[K] };
export type AddOptionalResult = AllOptional<RequiredSource>;

// ---------------------------------------------------------------------------
// (4) -? optional remover — PRESENCE-ONLY
//
// `-?` is a presence modifier: it clears the optional flag. The
// optional-origin `undefined` is carried by the flag (not in the value
// slot), so clearing the flag on an OPTIONAL-origin property naturally
// yields the bare value type. `OptionalSource` uses the `?` marker so
// `AllRequired<OptionalSource>` = `{ a: string; b: number }` (bare).
//
// The companion `ExplicitUndefinedSource` proves the dual: a property whose
// `| undefined` is EXPLICIT on a REQUIRED slot is preserved by `-?` (real TS:
// `Required<{ a: string | undefined }>` = `{ a: string | undefined }`), because
// the `undefined` is part of the declared type, not an optional-origin marker.
// ---------------------------------------------------------------------------
export type OptionalSource = {
  a?: string;
  b?: number;
};
export type AllRequired<T> = { [K in keyof T]-?: T[K] };
export type RemoveOptionalResult = AllRequired<OptionalSource>;

export type ExplicitUndefinedSource = {
  a: string | undefined;
};
export type RequiredExplicitUndefined = AllRequired<ExplicitUndefinedSource>;

// ---------------------------------------------------------------------------
// (5) Combined `-readonly -?`
// ---------------------------------------------------------------------------
export type ReadonlyOptionalSource = {
  readonly a?: string;
  readonly b?: number;
};
export type WritableRequired<T> = { -readonly [K in keyof T]-?: T[K] };
export type WritableRequiredResult = WritableRequired<ReadonlyOptionalSource>;

// ---------------------------------------------------------------------------
// (6) `as never` filter to drop keys whose name starts with "_"
// ---------------------------------------------------------------------------
export type FilterSource = {
  _internal: string;
  visible: number;
  _hidden: boolean;
};
export type DropPrivate<T> = {
  [K in keyof T as K extends `_${string}` ? never : K]: T[K];
};
export type DropPrivateResult = DropPrivate<FilterSource>;

// ---------------------------------------------------------------------------
// (7) Mapped type with conditional value expression
// ---------------------------------------------------------------------------
export type ValueSource = { a: string; b: number; c: "literal" };
export type StringValuesOnly<T> = {
  [K in keyof T]: T[K] extends string ? T[K] : never;
};
export type StringValuesOnlyResult = StringValuesOnly<ValueSource>;

// ---------------------------------------------------------------------------
// (8) Generic-constrained-key mapped (Pick2-style)
//
// The key union is a generic parameter constrained to `keyof T`. The mapped
// type instantiates `K = "a" | "c"` and projects only those members from `T`.
// Equivalent to TypeScript's built-in `Pick<T, K>`.
// ---------------------------------------------------------------------------
export type Pick2<T, K extends keyof T> = { [P in K]: T[P] };
export type Pick2Result = Pick2<{ a: number; b: string; c: boolean }, "a" | "c">;

// ---------------------------------------------------------------------------
// (9) Modifier idempotence — `+readonly` over an already-readonly source
//
// Applying the `+readonly` mapped form to a source whose members are already
// readonly is a no-op at the structural surface. Both members survive,
// readonly stays set, optional stays false.
// ---------------------------------------------------------------------------
export type AlreadyReadonly = { readonly a: string; readonly b: number };
export type ReadonlyOverReadonly = AllReadonly<AlreadyReadonly>;

// ---------------------------------------------------------------------------
// (10) `as` rename without filter (Capitalize-rename)
//
// Every key survives because `K extends string` holds for every string key in
// the source. The `Capitalize<K>` template literal helper rewrites each
// key to its capitalized form.
// ---------------------------------------------------------------------------
export type CapitalizeKeys<T> = {
  [K in keyof T as K extends string ? Capitalize<K> : never]: T[K];
};
export type CapitalizedResult = CapitalizeKeys<{ alpha: number; beta: string }>;
"#;

// The vendored source bytes of the `union_key_access` + `mode_boundary`
// re-export-chain fixtures (the registry is the source-byte authority).
// Inlined verbatim (PURE owned `&'static str`); the guard
// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
// byte-identity with the corresponding `fixtures/*.ts` files.

#[allow(dead_code)]
pub(crate) const UNION_KEY_ACCESS_SOURCE: &str = r#"// @ai-generated - Synthetic indexed-access-with-union-key typeinfo fixture.

export type Surface = {
  alpha: number;
  beta: string;
  gamma: boolean;
  delta: null;
};

export type AlphaBeta = Surface["alpha" | "beta"];
export type EveryMember = Surface[keyof Surface];

// Pick-style equivalent: Pick<Surface, "alpha" | "beta"> = { alpha; beta }.
export type PickAlphaBeta = Pick<Surface, "alpha" | "beta">;
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_PRINCIPAL_SOURCE: &str = r#"// @ai-generated - principal consumer of the 6-hop re-export chain.
// Imports `Foo` whose final definition (`{ b: 1 }`) lives 7 hops away in
// `mode_boundary_reexport_leaf.ts`. Mirrors the tsgo-audit benchmark
// re-export-chain shape.
//
// TS7 emission verified against tsgo 7.0.0-dev.20260523.1:
//   type WantedType = Foo & { a: 1 }
//   = { b: 1; } & { a: 1; } (structurally equivalent to `{ a: 1; b: 1 }`)
//   type WantedKeys = keyof WantedType
//   = "a" | "b"
import { Foo } from "./mode_boundary_reexport_link_1";

export type WantedType = Foo & { a: 1 };
export type WantedKeys = keyof WantedType;
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_LINK_1_SOURCE: &str = r#"// @ai-generated - hop 1 of the mode_boundary re-export chain (closest
// to the principal consumer).
export { Foo } from "./mode_boundary_reexport_link_2";
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_LINK_2_SOURCE: &str = r#"// @ai-generated - hop 2 of the mode_boundary re-export chain.
export { Foo } from "./mode_boundary_reexport_link_3";
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_LINK_3_SOURCE: &str = r#"// @ai-generated - hop 3 of the mode_boundary re-export chain.
export { Foo } from "./mode_boundary_reexport_link_4";
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_LINK_4_SOURCE: &str = r#"// @ai-generated - hop 4 of the mode_boundary re-export chain.
export { Foo } from "./mode_boundary_reexport_link_5";
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_LINK_5_SOURCE: &str = r#"// @ai-generated - hop 5 of the mode_boundary re-export chain.
export { Foo } from "./mode_boundary_reexport_link_6";
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_LINK_6_SOURCE: &str = r#"// @ai-generated - hop 6 of the mode_boundary re-export chain (uses
// `export *` to test wildcard barrel propagation).
export * from "./mode_boundary_reexport_barrel";
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_BARREL_SOURCE: &str = r#"// @ai-generated - barrel hop of the mode_boundary re-export chain.
export { Foo } from "./mode_boundary_reexport_leaf";
"#;

#[allow(dead_code)]
pub(crate) const MODE_BOUNDARY_REEXPORT_LEAF_SOURCE: &str = r#"// @ai-generated - terminal leaf of the mode_boundary re-export chain.
export type Foo = { b: 1 };
"#;

/// The workspace-file set the `union_key_access.rs` keyof-self-index row
/// upserts.
#[allow(dead_code)]
const UNION_KEY_ACCESS_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/union_key_access.ts",
    source: UNION_KEY_ACCESS_SOURCE,
}];

/// The 9-file workspace the `mode_boundary_invariants.rs` re-export keyof row
/// upserts: the principal consumer + the 6 link hops + the barrel + the leaf
/// (`Foo`'s terminal `{ b: 1 }` body lives 7 hops away).
#[allow(dead_code)]
const MODE_BOUNDARY_REEXPORT_FILES: &[WorkspaceFileSpec] = &[
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_principal.ts",
        source: MODE_BOUNDARY_REEXPORT_PRINCIPAL_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_link_1.ts",
        source: MODE_BOUNDARY_REEXPORT_LINK_1_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_link_2.ts",
        source: MODE_BOUNDARY_REEXPORT_LINK_2_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_link_3.ts",
        source: MODE_BOUNDARY_REEXPORT_LINK_3_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_link_4.ts",
        source: MODE_BOUNDARY_REEXPORT_LINK_4_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_link_5.ts",
        source: MODE_BOUNDARY_REEXPORT_LINK_5_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_link_6.ts",
        source: MODE_BOUNDARY_REEXPORT_LINK_6_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_barrel.ts",
        source: MODE_BOUNDARY_REEXPORT_BARREL_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/mode_boundary_reexport_leaf.ts",
        source: MODE_BOUNDARY_REEXPORT_LEAF_SOURCE,
    },
];

/// One `ResolveExpr`/`Expanded`/`DistributiveIdentity` query spec for a keyof
/// carve-out row (`keyof Root` / `Root[keyof Root]`). The shared resolver
/// reduces the keyof family root to its terminal (literal key union / member
/// value union); tsgo captures the SAME expansion through the
/// distributive-identity probe scaffold — applied UNIFORMLY to the admitted
/// keyof carve-out family, never branched on predicted display behavior.
const fn keyof_carve_out_spec(
    row_file: &'static str,
    row_function: &'static str,
    oracle_family: &'static str,
    workspace_files: &'static [WorkspaceFileSpec],
    primary_canonical: &'static str,
    symbol: &'static str,
) -> QuerySpec {
    QuerySpec {
        row_file,
        row_function,
        query_ordinal: 0,
        oracle_family,
        workspace_files,
        primary_canonical,
        host_project: HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol,
            type_args: &[],
            projection_mode: ProjectionModeSpec::Expanded,
            probe_rhs: ProbeRhsSpec::DistributiveIdentity,
        },
        source_locator: SourceLocatorSpec {
            reference_canonical: primary_canonical,
            reference_name: symbol,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// The workspace-file set the `mapped_modifiers.rs` `-?` row upserts.
#[allow(dead_code)]
const MAPPED_MODIFIERS_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/mapped_modifiers.ts",
    source: MAPPED_MODIFIERS_SOURCE,
}];

/// Vendored source bytes of `/fixtures/utility_top_bottom.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/utility_top_bottom.ts`.
#[allow(dead_code)]
pub(crate) const UTILITY_TOP_BOTTOM_SOURCE: &str = r#"// @ai-generated - Synthetic top/bottom-type utility fixture.
//
// Codifies TS7's behaviour for built-in utilities applied to top/bottom
// inputs (`any` / `unknown` / `never` / `null` / `undefined` / `void`).
// These are the "catches you when regular objects pass but degenerate
// inputs blow up" cases for the resolver's utility dispatch.
//
// Each alias is consumed through a corresponding Rust test in
// `utility_top_bottom.rs`.

// ============================================================
// ReturnType matrix
// ============================================================

// TS7: `any` (the conditional distributes over `any`, both branches
// contribute, and the merged result collapses to `any`).
export type Utb01ReturnTypeOfAny = ReturnType<any>;

// TS7: never (cannot extract a return from a bottom-typed callable)
export type Utb02ReturnTypeOfNever = ReturnType<never>;

// TS7: any
export type Utb03ReturnTypeAnyArrow = ReturnType<() => any>;

// TS7: never
export type Utb04ReturnTypeNeverArrow = ReturnType<() => never>;

// TS7: unknown
export type Utb05ReturnTypeUnknownArrow = ReturnType<() => unknown>;

// TS7: void
export type Utb06ReturnTypeVoidArrow = ReturnType<() => void>;

// ============================================================
// Parameters matrix
// ============================================================

// TS7: `unknown[]` — when T is `any`, the inferred `infer P` resolves
// against the constraint `(...args: any) => any`, yielding `unknown[]`
// (NOT `any` and NOT `never`). This is one of the trap cases.
export type Utb07ParametersOfAny = Parameters<any>;

// TS7: never
export type Utb08ParametersOfNever = Parameters<never>;

// TS7: [x: any]
export type Utb09ParametersAnyArg = Parameters<(x: any) => void>;

// TS7: [x: never]
export type Utb10ParametersNeverArg = Parameters<(x: never) => void>;

// ============================================================
// ConstructorParameters / InstanceType
// ============================================================

// TS7: `unknown[]` — like `Parameters<any>`, `ConstructorParameters<any>`
// reduces to the constraint's inferred tuple = `unknown[]`.
export type Utb11ConstructorParametersAny = ConstructorParameters<any>;

// TS7: any
export type Utb12InstanceTypeAny = InstanceType<any>;

// TS7: any[]
export type Utb13ConstructorParametersAnyCtor = ConstructorParameters<new (...args: any[]) => any>;

// ============================================================
// Awaited matrix
// ============================================================

// TS7: any
export type Utb14AwaitedAny = Awaited<any>;

// TS7: unknown
export type Utb15AwaitedUnknown = Awaited<unknown>;

// TS7: never
export type Utb16AwaitedNever = Awaited<never>;

// TS7: null
export type Utb17AwaitedNull = Awaited<null>;

// TS7: undefined
export type Utb18AwaitedUndefined = Awaited<undefined>;

// TS7: string (Awaited recursively unwraps nested Promises)
export type Utb19AwaitedNestedPromise = Awaited<Promise<Promise<string>>>;

// ============================================================
// NonNullable matrix
// ============================================================

// TS7: any
export type Utb20NonNullableAny = NonNullable<any>;

// TS7: {} (NonNullable<unknown> reduces to the empty-object base)
export type Utb21NonNullableUnknown = NonNullable<unknown>;

// TS7: never
export type Utb22NonNullableNever = NonNullable<never>;

// TS7: never (every constituent of the input is null or undefined)
export type Utb23NonNullableNullableOnly = NonNullable<null | undefined>;

// ============================================================
// Extract / Exclude matrix
// ============================================================

// TS7: any
export type Utb24ExtractAnyAgainstString = Extract<any, string>;

// TS7: any
export type Utb25ExcludeAnyAgainstString = Exclude<any, string>;

// TS7: never (distributing over `never` collapses)
export type Utb26ExtractNeverAgainstString = Extract<never, string>;

// TS7: never
export type Utb27ExcludeNeverAgainstString = Exclude<never, string>;

// TS7: never (`unknown extends string` is false, so `T extends U ? T : never`
// collapses to `never`).
export type Utb28ExtractUnknownAgainstString = Extract<unknown, string>;

// TS7: unknown
export type Utb29ExcludeUnknownAgainstString = Exclude<unknown, string>;
"#;

/// The workspace-file set the `utility_top_bottom.rs` rows upsert.
#[allow(dead_code)]
const UTILITY_TOP_BOTTOM_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/utility_top_bottom.ts",
    source: UTILITY_TOP_BOTTOM_SOURCE,
}];

/// Vendored source bytes of `/fixtures/variadic_tuples.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/variadic_tuples.ts`.
#[allow(dead_code)]
pub(crate) const VARIADIC_TUPLES_SOURCE: &str = r#"// @ai-generated - Synthetic variadic-tuple typeinfo fixture.

export type Head<T extends readonly unknown[]> = T extends readonly [infer H, ...unknown[]]
  ? H
  : never;
export type Tail<T extends readonly unknown[]> = T extends readonly [unknown, ...infer R] ? R : [];
export type Last<T extends readonly unknown[]> = T extends readonly [...unknown[], infer L]
  ? L
  : never;
export type Init<T extends readonly unknown[]> = T extends readonly [...infer I, unknown] ? I : [];
export type Concat<A extends readonly unknown[], B extends readonly unknown[]> = [...A, ...B];

export type SampleTuple = [1, 2, 3];

export type HeadOfSample = Head<SampleTuple>;
export type TailOfSample = Tail<SampleTuple>;
export type LastOfSample = Last<SampleTuple>;
export type InitOfSample = Init<SampleTuple>;
export type ConcatPair = Concat<[1, 2], [3, 4]>;

// Variadic in a function signature
export declare function variadic<A extends readonly unknown[], B extends readonly unknown[]>(
  a: [...A],
  b: [...B],
): [...A, ...B];

export type VariadicCallResult = ReturnType<typeof variadic<[1, 2], [3, 4]>>;
"#;

/// The workspace-file set the `variadic_tuples.rs` rows upsert.
#[allow(dead_code)]
const VARIADIC_TUPLES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/variadic_tuples.ts",
    source: VARIADIC_TUPLES_SOURCE,
}];

/// Vendored source bytes of `/fixtures/utility-composition.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/utility_composition.ts`.
#[allow(dead_code)]
pub(crate) const UTILITY_COMPOSITION_SOURCE: &str = r#"// @ai-generated - Synthetic utility-composition typeinfo fixture.

export interface UtilitySource {
  id: string;
  label?: string;
  tone?: "neutral" | "accent" | "danger";
  mode: "view" | "edit" | "debug";
  internal?: {
    trace: boolean;
    sink: (event: string) => void;
  };
  payload?: {
    count?: number;
    tags?: string[];
  };
}

export type RequiredIdentity = Required<Pick<UtilitySource, "id" | "label">>;
export type PublicPartial = Partial<Omit<UtilitySource, "internal">>;
export type VisibleMode = Extract<UtilitySource["mode"], "view" | "edit">;
export type RuntimeMode = Exclude<UtilitySource["mode"], "debug">;
export type UtilityCombinationSurface = RequiredIdentity &
  PublicPartial & {
    visibleMode: VisibleMode;
    runtimeMode: RuntimeMode;
  };

export type DeepUtilityPayload = Required<
  Pick<NonNullable<UtilitySource["payload"]>, "count" | "tags">
>;

export type DeepUtilityConfig = Required<
  Pick<Partial<Omit<UtilitySource, "internal">>, "mode" | "payload">
> & {
  mode: Extract<UtilitySource["mode"], "view" | "edit">;
  tone: Exclude<NonNullable<UtilitySource["tone"]>, "danger">;
  payload: DeepUtilityPayload;
};
"#;

/// The closed registry table. Holds the 19 lifted rows — the two
/// index-signature publication queries, the two built-in modifier-utility
/// queries, the three U2 IndexedAccess-reduction carve-out queries (two
/// terminal indexed-access projections + one wide/deep literal-union
/// projection), the mapped-modifier `-?` query, the three keyof-expansion
/// carve-out queries captured through the distributive-identity scaffold,
/// and the eight U2.UTILITIES reducer queries (five Awaited rows, two
/// NonNullable rows, and the variadic-spread Concat row)
/// (`docs/arch/ts-compat-two-mode-model.md`, `docs/arch/u0-oracle-harness-design.md`).
#[allow(dead_code)]
pub(crate) const ORACLE_QUERY_SPECS: &[QuerySpec] = &[
    index_signature_publication_spec(
        "index_signatures_numeric_index_publishes_signature",
        "NumericIndexed",
    ),
    index_signature_publication_spec(
        "index_signatures_symbol_index_publishes_signature",
        "SymbolIndexed",
    ),
    utility_edge_modifier_spec(
        "utility_edge_required_strips_optional_markers",
        "RequiredOptional",
    ),
    utility_edge_modifier_spec(
        "utility_edge_readonly_required_composes_modifiers",
        "ReadonlyRequiredOptional",
    ),
    carve_out_spec(
        "typescript_rules.rs",
        "typescript_rules_indexed_access_reduces_terminal_property",
        "typescript_rules",
        TYPESCRIPT_RULES_FILES,
        "/fixtures/typescript-rules.ts",
        "IndexedRules",
    ),
    carve_out_spec(
        "deep_path.rs",
        "deep_path_projection_resolves_terminal_without_losing_shape",
        "deep_path",
        DEEP_PATH_FILES,
        "/fixtures/deep-path.ts",
        "DeepProjectedTarget",
    ),
    carve_out_spec(
        "wide_deep.rs",
        "wide_deep_projected_token_resolves_literal_union",
        "wide_deep",
        WIDE_DEEP_FILES,
        "/fixtures/wide-deep.ts",
        "WideDeepProjectedToken",
    ),
    carve_out_spec(
        "mapped_modifiers.rs",
        "mapped_modifier_minus_optional_strips_optional_and_undefined",
        "mapped_modifiers",
        MAPPED_MODIFIERS_FILES,
        "/fixtures/mapped_modifiers.ts",
        "RemoveOptionalResult",
    ),
    keyof_carve_out_spec(
        "typescript_rules.rs",
        "typescript_rules_keyof_materializes_literal_key_union",
        "typescript_rules",
        TYPESCRIPT_RULES_FILES,
        "/fixtures/typescript-rules.ts",
        "KeyOfRules",
    ),
    keyof_carve_out_spec(
        "mode_boundary_invariants.rs",
        "mode_boundary_keyof_across_reexport_chain_resolves_all_keys",
        "mode_boundary_invariants",
        MODE_BOUNDARY_REEXPORT_FILES,
        "/fixtures/mode_boundary_reexport_principal.ts",
        "WantedKeys",
    ),
    keyof_carve_out_spec(
        "union_key_access.rs",
        "union_key_access_keyof_self_projects_full_value_union",
        "union_key_access",
        UNION_KEY_ACCESS_FILES,
        "/fixtures/union_key_access.ts",
        "EveryMember",
    ),
    carve_out_spec(
        "utility_top_bottom.rs",
        "utility_top_bottom_utb17_awaited_null_is_null",
        "utility_top_bottom",
        UTILITY_TOP_BOTTOM_FILES,
        "/fixtures/utility_top_bottom.ts",
        "Utb17AwaitedNull",
    ),
    carve_out_spec(
        "utility_top_bottom.rs",
        "utility_top_bottom_utb18_awaited_undefined_is_undefined",
        "utility_top_bottom",
        UTILITY_TOP_BOTTOM_FILES,
        "/fixtures/utility_top_bottom.ts",
        "Utb18AwaitedUndefined",
    ),
    carve_out_spec(
        "utility_top_bottom.rs",
        "utility_top_bottom_utb19_awaited_nested_promise_is_inner_primitive",
        "utility_top_bottom",
        UTILITY_TOP_BOTTOM_FILES,
        "/fixtures/utility_top_bottom.ts",
        "Utb19AwaitedNestedPromise",
    ),
    carve_out_spec(
        "typescript_rules.rs",
        "typescript_rules_awaited_recursively_unwraps_promises",
        "typescript_rules",
        TYPESCRIPT_RULES_FILES,
        "/fixtures/typescript-rules.ts",
        "AwaitedRules",
    ),
    carve_out_spec(
        "utility_edge.rs",
        "utility_edge_non_nullable_strips_null_and_undefined",
        "utility_edge",
        UTILITY_EDGE_FILES,
        "/fixtures/utility_edge.ts",
        "NonNullablePrim",
    ),
    carve_out_spec(
        "variadic_tuples.rs",
        "variadic_tuple_concat_alias_produces_joined_literal_tuple",
        "variadic_tuples",
        VARIADIC_TUPLES_FILES,
        "/fixtures/variadic_tuples.ts",
        "ConcatPair",
    ),
    carve_out_spec(
        "utility_top_bottom.rs",
        "utility_top_bottom_utb21_non_nullable_unknown_is_empty_object",
        "utility_top_bottom",
        UTILITY_TOP_BOTTOM_FILES,
        "/fixtures/utility_top_bottom.ts",
        "Utb21NonNullableUnknown",
    ),
    carve_out_spec(
        "utility_top_bottom.rs",
        "utility_top_bottom_utb15_awaited_unknown_is_unknown",
        "utility_top_bottom",
        UTILITY_TOP_BOTTOM_FILES,
        "/fixtures/utility_top_bottom.ts",
        "Utb15AwaitedUnknown",
    ),
];

/// Why the registry is malformed. Pure value type (no external dependency) so
/// the validation is shareable into both consumers.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistryError {
    /// An entry carries an empty `oracle_family` (it could not name a snapshot
    /// sub-directory).
    EmptyOracleFamily {
        row_file: &'static str,
        row_function: &'static str,
        query_ordinal: u16,
    },
    /// A row's `query_ordinal`s are not the unique, contiguous `0..count-1` set
    /// the §Q4 cross-check requires (a gap, a duplicate, or an off-by-one).
    NonContiguousOrdinals {
        row_file: &'static str,
        row_function: &'static str,
    },
}

/// Validate the registry's structural well-formedness (§Q4) — PURE, over the
/// registry's own types only: (a) every entry carries a non-empty
/// `oracle_family`; (b) each `(row_file, row_function)` row's `query_ordinal`s
/// are UNIQUE and CONTIGUOUS `0..count-1` (no gap / duplicate / off-by-one). The
/// independent declared-count cross-check (the DEFERRED §Q4 per-row-count layer's
/// `oracle_query_ordinals` count, not yet a shipped `IgnoredTestRow` field) is a
/// separate, not-yet-wired concern — see `docs/arch/u0-oracle-harness-design.md` §Q4.
#[allow(dead_code)]
pub(crate) fn registry_well_formed(specs: &[QuerySpec]) -> Result<(), RegistryError> {
    for spec in specs {
        if spec.oracle_family.is_empty() {
            return Err(RegistryError::EmptyOracleFamily {
                row_file: spec.row_file,
                row_function: spec.row_function,
                query_ordinal: spec.query_ordinal,
            });
        }
    }

    // Group ordinals per (row_file, row_function) and check contiguity.
    let mut rows: Vec<(&'static str, &'static str)> = Vec::new();
    for spec in specs {
        let key = (spec.row_file, spec.row_function);
        if !rows.contains(&key) {
            rows.push(key);
        }
    }
    for (row_file, row_function) in rows {
        let mut ordinals: Vec<u16> = specs
            .iter()
            .filter(|s| s.row_file == row_file && s.row_function == row_function)
            .map(|s| s.query_ordinal)
            .collect();
        ordinals.sort_unstable();
        for (expected, got) in ordinals.iter().enumerate() {
            if u16::try_from(expected).ok() != Some(*got) {
                return Err(RegistryError::NonContiguousOrdinals {
                    row_file,
                    row_function,
                });
            }
        }
    }

    Ok(())
}
