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
// The registry seats the 7 lifted rows (the two index-signature
// publication queries + the two built-in modifier-utility queries + the three
// U2 IndexedAccess-reduction carve-out queries). The types + validation are
// additionally exercised with synthetic specs by the discriminating guards.

/// The content id of the CURRENT closed vendored oracle-env corpus — the
/// pinned-env constant the registry + every guard read to derive a
/// `snapshot_id` and locate `oracle_env/<env_corpus_id>/` WITHOUT opening a
/// snapshot (§Q4). It is EMPTY until the snapshot generator first vendors the
/// corpus and pins it; while empty no real snapshot exists, so
/// no real `snapshot_id` is derived on the consumption path.
#[allow(dead_code)]
pub(crate) const CURRENT_ENV_CORPUS_ID: &str =
    "blake3:c6c4bda7c5c5106e873a66c7da516f6a0545492e280dfb9e964f833ed0e8d8f7";

/// Stable hash of the EFFECTIVE canonical `oracle.tsconfig.json` (§Q2 "Env
/// pinning"). EMPTY until the snapshot generator vendors the canonical config
/// and computes it.
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

/// Which `support.rs` helper produces the in-process `TypeExpr`, with its
/// kind-specific payload (§Q4). The `*Expr` suffix mirrors the design-mandated
/// helper names; the shared suffix is intentional, not a naming smell.
#[allow(dead_code, clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryHelperSpec {
    /// `resolve_expr(host, canonical, symbol, type_args, mode)`. `type_args` is
    /// the canonical `TypeExpr`-JSON of each argument (empty for the common
    /// non-generic case; non-empty rows are deferred until the printer spike).
    ResolveExpr {
        symbol: &'static str,
        type_args: &'static [&'static str],
        projection_mode: ProjectionModeSpec,
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
        },
        source_locator: SourceLocatorSpec {
            reference_canonical: primary_canonical,
            reference_name: symbol,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// The closed registry table. Holds the lifted rows — the two index-signature
/// publication queries, the two built-in modifier-utility queries, and the
/// three U2 IndexedAccess-reduction carve-out queries (two terminal indexed-access
/// projections + one wide/deep literal-union projection)
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
