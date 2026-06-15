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
// The registry seats the 46 lifted rows; the authoritative enumeration lives
// on `ORACLE_QUERY_SPECS`' doc comment and is pinned exactly by
// `oracle_query_specs_registry_holds_the_lifted_rows_and_is_well_formed`. The
// types + validation are additionally exercised with synthetic specs by the
// discriminating guards.

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

include!("oracle_query_specs_vendored_sources.rs");
include!("oracle_query_specs_vendored_sources_module_aug.rs");
include!("oracle_query_specs_vendored_sources_jsx.rs");
include!("oracle_query_specs_vendored_sources_mapped_template.rs");

/// The single-file workspace the JSX lift rows upsert (the `jsx.ts` fixture
/// declares the global `JSX` namespace through `declare global { namespace JSX
/// { ... } }` blocks; every JSX lift row resolves a standalone alias against it).
#[allow(dead_code)]
const JSX_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/jsx.ts",
    source: JSX_SOURCE,
}];

/// The single-file workspace the `mapped_template.rs` `RecordTemplateRootSlot`
/// lift row upserts. The on-disk fixture name uses an underscore but the row
/// upserts it at the hyphenated canonical path `/fixtures/mapped-template.ts`
/// (the canonical upsert path is independent of the on-disk fixture filename).
#[allow(dead_code)]
const MAPPED_TEMPLATE_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/mapped-template.ts",
    source: MAPPED_TEMPLATE_SOURCE,
}];

/// The single-file workspace the `template_literal_inference.rs`
/// `CounterHandlers` lift row upserts.
#[allow(dead_code)]
const TEMPLATE_LITERAL_INFERENCE_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/template_literal_inference.ts",
    source: TEMPLATE_LITERAL_INFERENCE_SOURCE,
}];

/// The closed registry table. Holds the 46 lifted rows — the two
/// index-signature publication queries, the two built-in modifier-utility
/// queries, the three U2 IndexedAccess-reduction carve-out queries (two
/// terminal indexed-access projections + one wide/deep literal-union
/// projection), the mapped-modifier `-?` query, the three keyof-expansion
/// carve-out queries captured through the distributive-identity scaffold,
/// the eight U2.UTILITIES reducer queries (five Awaited rows, two
/// NonNullable rows, and the variadic-spread Concat row), the nineteen
/// U2.CLASS_SURFACES-era queries (two brand-tag index chains, three
/// class-features static rows, nine function-advanced
/// signature-bucket/prototype/overload rows, the sb15 bare-generic
/// ReturnType row, two typescript-rules construct-signature rows, two
/// decoration-invariance rows), and the four U2.MODULE_AUGMENTATION-era
/// queries (the `as const` typeof indexed member + the two `typeof import(...)`
/// value-member projections [named-value + default-export shape] at
/// `U2.INDEXED_ACCESS`, plus the namespace alias-chain projection at
/// `U2.QUERY_VALUE_DOMAIN`), the two U2.JSX-era queries (the two
/// parametric `IntrinsicPropsFor<"div">` / `IntrinsicPropsFor<"span">`
/// intrinsic-lookup rows whose `JSX.IntrinsicElements[Tag]` reduction
/// terminates at `IndexedAccess` and sits under `U2.INDEXED_ACCESS`), and the
/// two U2.MAPPED_TEMPLATE-era queries (the `RecordTemplateRootSlot` same-file
/// string-literal index-chain row + the `CounterHandlers` `Capitalize` key-remap
/// mapped-type row, both terminating at `MappedTemplateRemap` under
/// `U2.MAPPED_TEMPLATE`)
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
    carve_out_spec(
        "branded_types.rs",
        "branded_key_access_projects_literal_brand_tag",
        "branded_types",
        BRANDED_TYPES_FILES,
        "/fixtures/branded_types.ts",
        "UserIdBrandTag",
    ),
    carve_out_spec(
        "branded_types.rs",
        "branded_key_access_projects_boolean_literal_brand_tag",
        "branded_types",
        BRANDED_TYPES_FILES,
        "/fixtures/branded_types.ts",
        "CentsBrandTag",
    ),
    carve_out_spec(
        "class_features.rs",
        "class_features_static_inheritance_resolves_inherited_field_type",
        "class_features",
        CLASS_FEATURES_FILES,
        "/fixtures/class_features.ts",
        "StepCounterInitial",
    ),
    carve_out_spec(
        "class_features.rs",
        "class_features_static_inheritance_resolves_inherited_method_return",
        "class_features",
        CLASS_FEATURES_FILES,
        "/fixtures/class_features.ts",
        "StepCounterDescribeReturn",
    ),
    carve_out_spec(
        "class_features.rs",
        "class_features_static_generic_method_instantiation_projects_return_with_substitution",
        "class_features",
        CLASS_FEATURES_FILES,
        "/fixtures/class_features.ts",
        "StaticMethodInstantiated",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_constructor_parameters_publishes_constructor_arg_tuple",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "CtorParams",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_instance_type_publishes_constructor_return_shape",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "CtorInstance",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_call_construct_hybrid_parameters_uses_call_signature",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "CallableCallParams",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_call_construct_hybrid_return_type_uses_call_signature",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "CallableCallReturn",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "CallableCtorParams",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_call_construct_hybrid_instance_type_uses_construct_signature",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "CallableCtorInstance",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_class_method_prototype_extraction_projects_return",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "ExtractedGreetReturn",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_class_method_prototype_extraction_projects_parameters",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "ExtractedGreetParams",
    ),
    carve_out_spec(
        "function_advanced.rs",
        "function_advanced_return_type_of_overloaded_function_uses_last_overload",
        "function_advanced",
        FUNCTION_ADVANCED_FILES,
        "/fixtures/function_advanced.ts",
        "LookupReturnType",
    ),
    carve_out_spec(
        "substitution_types.rs",
        "substitution_types_sb15_recursive_generic_substitution",
        "substitution_types",
        SUBSTITUTION_TYPES_FILES,
        "/fixtures/substitution_types.ts",
        "Sb15Result",
    ),
    carve_out_spec(
        "typescript_rules.rs",
        "typescript_rules_constructor_parameters_resolve_tuple",
        "typescript_rules",
        TYPESCRIPT_RULES_FILES,
        "/fixtures/typescript-rules.ts",
        "ConstructorParamsRules",
    ),
    carve_out_spec(
        "typescript_rules.rs",
        "typescript_rules_instance_type_resolves_constructed_object",
        "typescript_rules",
        TYPESCRIPT_RULES_FILES,
        "/fixtures/typescript-rules.ts",
        "InstanceRules",
    ),
    carve_out_spec(
        "decorators.rs",
        "decorators_identity_method_decorator_preserves_return_inference",
        "decorators",
        DECORATORS_FILES,
        "/fixtures/decorators.ts",
        "MethodHostTagReturn",
    ),
    carve_out_spec(
        "decorators.rs",
        "decorators_metadata_reader_describe_return_is_literal_union",
        "decorators",
        DECORATORS_FILES,
        "/fixtures/decorators.ts",
        "MetadataAwareDescribeReturn",
    ),
    // U2.MODULE_AUGMENTATION-era lifts. The four rows whose measured dispatch
    // trace re-homes them OUT of `U2.MODULE_AUGMENTATION`: the `as const`
    // typeof indexed member + the two `typeof import(...)["…"]` value-member
    // projections terminate at `IndexedAccess` (`U2.INDEXED_ACCESS`); the
    // namespace alias-chain row dispatches only `ResolveDecl` + `Instantiate`
    // (`U2.QUERY_VALUE_DOMAIN`). Each resolves a single declared type alias
    // through the shared five-mode dispatch; tsgo expands the same alias.
    carve_out_spec(
        "modern_ts_features.rs",
        "import_attribute_simulated_string_literal_indexed_member",
        "modern_ts_features",
        MODERN_TS_FEATURES_FILES,
        "/fixtures/modern_ts_features.ts",
        "ImportedJsonName",
    ),
    carve_out_spec(
        "module_features.rs",
        "module_features_namespace_geometry_vector_aliases_point",
        "module_features",
        MODULE_FEATURES_MAIN_FILES,
        "/fixtures/module_features.ts",
        "GeometryVector",
    ),
    carve_out_spec(
        "module_features.rs",
        "module_features_typeof_import_named_value_resolves_to_literal",
        "module_features",
        MODULE_FEATURES_CONSUMER_FILES,
        "/fixtures/module_features_consumer.ts",
        "LeafNamedValue",
    ),
    carve_out_spec(
        "module_features.rs",
        "module_features_typeof_import_default_resolves_value_shape",
        "module_features",
        MODULE_FEATURES_CONSUMER_FILES,
        "/fixtures/module_features_consumer.ts",
        "LeafDefault",
    ),
    // U2.JSX-era lifts. The two parametric intrinsic-lookup rows
    // (`IntrinsicPropsFor<"div">` / `IntrinsicPropsFor<"span">`) whose source
    // body is a bare `Ref` carrying a string-literal type argument — the source
    // side admits (the walk does NOT descend the alias body) and tsgo expands
    // the alias to the declared intrinsic shape. The seven sibling JSX rows are
    // honest-deferred: rows 1/2/4/8 reject `DeferredConstruct("indexed-access")`,
    // row 6 rejects `DeferredConstruct("keyof")`, row 9 rejects
    // `EnumMemberOrQualified`, and row 3 fails the resolver preflight (the
    // `typeof createElement<…>` value-side generic instantiation is a U6
    // `ResolveCall` gap). Re-homed per the measured dispatch trace.
    carve_out_spec(
        "jsx.rs",
        "jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape",
        "jsx",
        JSX_FILES,
        "/fixtures/jsx.ts",
        "DivPropsViaIndex",
    ),
    carve_out_spec(
        "jsx.rs",
        "jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape",
        "jsx",
        JSX_FILES,
        "/fixtures/jsx.ts",
        "SpanPropsViaIndex",
    ),
    // U2.MAPPED_TEMPLATE-era lifts (tsgo-available oracle lift). Both source
    // bodies admit on the source side and reduce to a clean callable surface
    // tsgo expands structurally:
    //   - `RecordTemplateRootSlot = RecordTemplateSlots["slot:root"]` indexes a
    //     `Record<`slot:${"root"|"item"}`, …>` by the same-file string-literal
    //     key `slot:root` (the string-literal index-chain source-walk carve-out)
    //     and reduces to `(payload: { name: "item" | "root" }) => VNode[]`.
    //   - `CounterHandlers = EventHandlers<"inc" | "dec">` is a bare `Ref` over a
    //     string-literal union argument; tsgo expands the key-remapped mapped
    //     type to `{ onDec: (payload: "dec") => void; onInc: (payload: "inc") =>
    //     void }`.
    carve_out_spec(
        "mapped_template.rs",
        "record_with_template_literal_key_union_projects_root_slot",
        "mapped_template",
        MAPPED_TEMPLATE_FILES,
        "/fixtures/mapped-template.ts",
        "RecordTemplateRootSlot",
    ),
    carve_out_spec(
        "template_literal_inference.rs",
        "template_literal_key_remap_capitalises_each_event_key",
        "template_literal_inference",
        TEMPLATE_LITERAL_INFERENCE_FILES,
        "/fixtures/template_literal_inference.ts",
        "CounterHandlers",
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
/// independent declared-count cross-check against the row's `oracle_query_ordinals`
/// count — on BOTH the `IgnoredTestRow` manifest field AND the retained
/// `LiftMigrationProvenance` — is the separate, NOW-SHIPPED
/// `registry_entry_count_matches_declared` guard (§Q4).
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

// ---------------------------------------------------------------------------
// Retained-lift migration metadata (§Q4 — `migration_fingerprint` /
// `original_body_tokens`)
// ---------------------------------------------------------------------------
//
// The SOLE migration-fidelity authority for each lifted row: produced ONCE at
// lift time by the closed `syn` extractor over the ORIGINAL pre-`#[oracle_row]`
// body, retained here, and re-audited hermetically. The registry payload above
// is validated AGAINST this (`registry_payload_matches_migration_fingerprint`),
// never the reverse. `original_body_tokens` is the canonicalized (span-stripped,
// comment-free, path-const-folded) `syn::Block` token stream — the EXACT bytes
// the extractor reads — so `original_extraction_input_auditable` re-derives the
// fingerprint from this table alone, with NO VCS archaeology. NOT a `snapshot_id`
// input. Generated by the audited lift command; never hand-edited.

/// One lifted row's retained migration provenance.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftMigrationProvenance {
    pub(crate) row_file: &'static str,
    pub(crate) row_function: &'static str,
    /// The number of oracle queries the original body issued (= the row's
    /// registry-entry count). Cross-checked by `registry_entry_count_matches_declared`.
    pub(crate) oracle_query_ordinals: u16,
    pub(crate) migration_fingerprint_version: u32,
    pub(crate) migration_fingerprint: &'static str,
    /// The row's workspace file set CAPTURED at lift time, each
    /// `(canonical_path, content_hash)` SORTED by path (the `sha256:`
    /// `identity::content_hash` recipe). NOT body-token-extractable (the source
    /// routes through file-local consts / `upsert*` wrappers), so it is retained
    /// HERE and folded into `migration_fingerprint`; `original_extraction_input_auditable`
    /// re-derives the fingerprint from `original_body_tokens` + THIS axis alone, and
    /// `snapshot_workspace_files_match_retained_provenance` pins it to the snapshot's
    /// generator-written `identity.workspace_files`.
    pub(crate) workspace_files: &'static [(&'static str, &'static str)],
    pub(crate) original_body_tokens: &'static str,
}

/// The retained migration provenance for every lifted row. Looked up by
/// `(row_file, row_function)`; the lib `oracle-gen` regen mirrors each row's
/// `migration_fingerprint` (+ version) into its v3 snapshot, and the `tests/`
/// guards validate the registry payload + each snapshot against it.
#[allow(dead_code)]
pub(crate) const LIFTED_ROW_MIGRATIONS: &[LiftMigrationProvenance] = &[
    LiftMigrationProvenance {
        row_file: "branded_types.rs",
        row_function: "branded_key_access_projects_boolean_literal_brand_tag",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:38bfe4956de671d5b284cd4efc20e53aa3ee6cda57fb0ae9b525b13bc6385f1a",
        workspace_files: &[("/fixtures/branded_types.ts", "sha256:c7faceeb39c4f1dbf8b8fa200c1fe02cff1c527b8476ebb65f0882149f74d880")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/branded_types.ts\" , \"CentsBrandTag\" , & [] , ProjectionMode :: Expanded ,) ; assert_boolean_literal (& expr , true) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "branded_types.rs",
        row_function: "branded_key_access_projects_literal_brand_tag",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:8bff6f3ab01e93a8472af24cebf97cc9810f96d1a8fda4b91402c8e686dab56a",
        workspace_files: &[("/fixtures/branded_types.ts", "sha256:c7faceeb39c4f1dbf8b8fa200c1fe02cff1c527b8476ebb65f0882149f74d880")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/branded_types.ts\" , \"UserIdBrandTag\" , & [] , ProjectionMode :: Expanded ,) ; assert_string_literal (& expr , \"UserId\") ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "class_features.rs",
        row_function: "class_features_static_generic_method_instantiation_projects_return_with_substitution",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:c7252dff69623cc6953d633e68ca4ca1863dc0bd70a8bb668d0b4f14ac619cb7",
        workspace_files: &[("/fixtures/class_features.ts", "sha256:72b7927d9a0d5c3d197227b5bf874a6a8594963ccab65fd8531ad0e8c4554ebb")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/class_features.ts\" , \"StaticMethodInstantiated\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"wrapped\"]) ; assert_primitive (& props [\"wrapped\"] . ty , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "class_features.rs",
        row_function: "class_features_static_inheritance_resolves_inherited_field_type",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:bab2a7fe3a36a55b5336b88e84b750841a9f3c586979974bd03baf0e78dbf336",
        workspace_files: &[("/fixtures/class_features.ts", "sha256:72b7927d9a0d5c3d197227b5bf874a6a8594963ccab65fd8531ad0e8c4554ebb")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/class_features.ts\" , \"StepCounterInitial\" , & [] , ProjectionMode :: Expanded ,) ; assert_primitive (& expr , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "class_features.rs",
        row_function: "class_features_static_inheritance_resolves_inherited_method_return",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:fe1ccef3736d08d80d7b87ec40d262694fe53f28b6cd8c58a76252fcd3921a61",
        workspace_files: &[("/fixtures/class_features.ts", "sha256:72b7927d9a0d5c3d197227b5bf874a6a8594963ccab65fd8531ad0e8c4554ebb")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/class_features.ts\" , \"StepCounterDescribeReturn\" , & [] , ProjectionMode :: Expanded ,) ; assert_primitive (& expr , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "decorators.rs",
        row_function: "decorators_identity_method_decorator_preserves_return_inference",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:70ae03df0624621fdea31f797aeef9265fb0934c53953ca8000e6de6ef6d611e",
        workspace_files: &[("/fixtures/decorators.ts", "sha256:a50c1645c8720f9c45c79d617bd7c69ac6c34f3ca7406a642849aacb77d1f25b")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/decorators.ts\" , \"MethodHostTagReturn\" , & [] , ProjectionMode :: Expanded ,) ; assert_string_literal (& expr , \"tag\") ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "decorators.rs",
        row_function: "decorators_metadata_reader_describe_return_is_literal_union",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:6b09289967026d7c5f09567d2a8e9c0efd0231698946974b1490c00a42831333",
        workspace_files: &[("/fixtures/decorators.ts", "sha256:a50c1645c8720f9c45c79d617bd7c69ac6c34f3ca7406a642849aacb77d1f25b")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/decorators.ts\" , \"MetadataAwareDescribeReturn\" , & [] , ProjectionMode :: Expanded ,) ; assert_literal_union (& expr , & [\"pending\" , \"ready\"]) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "deep_path.rs",
        row_function: "deep_path_projection_resolves_terminal_without_losing_shape",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:f0f9e1be0d6808ecb3763dc47ba30fa08cdfb0dde0e0c5efe864d0167a7a1838",
        workspace_files: &[("/fixtures/deep-path.ts", "sha256:e63c47c47bf7af1532ba5bdf1faf1eb5b05af99df13d80c4406b800a12f4b1dd")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/deep-path.ts\" , DEEP_PATH) ; let (expr , record) = resolve_expr (& host , \"/fixtures/deep-path.ts\" , \"DeepProjectedTarget\" , & [] , ProjectionMode :: Expanded ,) ; let target = object_props (& expr) ; assert_primitive (& target [\"id\"] . ty , PrimitiveName :: String) ; assert_number_literal_union (& target [\"priority\"] . ty , & [1.0 , 2.0 , 3.0]) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:2a1130be6ac7c18d617f5a4cb502cd0aef6fb30094f91f765ec772ce9ab9bc0c",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"CallableCtorParams\" , & [] , ProjectionMode :: Expanded ,) ; let TypeExpr :: Tuple { elements , .. } = & expr else { panic ! (\"expected tuple, got {expr:?}\") ; } ; assert_eq ! (elements . len () , 1) ; assert_primitive (& elements [0] . ty , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_call_construct_hybrid_instance_type_uses_construct_signature",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:a500da2be1e602e01ab1d8df458b5b1619ad79467ebd21e19384a328d63b6b19",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"CallableCtorInstance\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"value\"]) ; assert_primitive (& props [\"value\"] . ty , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_call_construct_hybrid_parameters_uses_call_signature",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:6ed7a9f3e007f933500649efaaa835cb9a44c76aa94a42fea143897254e74d91",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"CallableCallParams\" , & [] , ProjectionMode :: Expanded ,) ; let TypeExpr :: Tuple { elements , .. } = & expr else { panic ! (\"expected tuple, got {expr:?}\") ; } ; assert_eq ! (elements . len () , 1) ; assert_primitive (& elements [0] . ty , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_call_construct_hybrid_return_type_uses_call_signature",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:e45e57fa8fd5218c12b0048572b26e335dca9140454604c5643fc26db6f90f80",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"CallableCallReturn\" , & [] , ProjectionMode :: Expanded ,) ; assert_primitive (& expr , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_class_method_prototype_extraction_projects_parameters",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:4eeb264cde7aac0301e556f86ac0b5b2cfd5f2a54e16e1cde590aceaaf7f5b32",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"ExtractedGreetParams\" , & [] , ProjectionMode :: Expanded ,) ; let TypeExpr :: Tuple { elements , .. } = & expr else { panic ! (\"expected tuple, got {expr:?}\") ; } ; assert_eq ! (elements . len () , 1) ; assert_primitive (& elements [0] . ty , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_class_method_prototype_extraction_projects_return",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:36d4b9d5ceb178b142858b1154c1e9e382cbe8cc3e9f9252d6708c6d32d2fb47",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"ExtractedGreetReturn\" , & [] , ProjectionMode :: Expanded ,) ; assert_primitive (& expr , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_constructor_parameters_publishes_constructor_arg_tuple",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:3a4180fae171eb46df4686e57f2a4658eac026852969373f5a233a5413b83aa2",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"CtorParams\" , & [] , ProjectionMode :: Expanded ,) ; let TypeExpr :: Tuple { elements , .. } = & expr else { panic ! (\"expected tuple, got {expr:?}\") ; } ; assert_eq ! (elements . len () , 1) ; assert_primitive (& elements [0] . ty , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_instance_type_publishes_constructor_return_shape",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:c6eb5e37f0eb40aaea3a6d40c7437f3dbf8a259bf08f10dfdb6c8604c4052e30",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"CtorInstance\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"id\" , \"ready\"]) ; assert_primitive (& props [\"id\"] . ty , PrimitiveName :: String) ; assert_primitive (& props [\"ready\"] . ty , PrimitiveName :: Boolean) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "function_advanced.rs",
        row_function: "function_advanced_return_type_of_overloaded_function_uses_last_overload",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:e05273507835885d31a4f2d86e59ed363f26e79f86d68f61b34eaefb6402c56f",
        workspace_files: &[("/fixtures/function_advanced.ts", "sha256:8e0e2b23c6f07b63f4864f4fc4c4799d658e2d2188c71e67e72ee15b90cf354c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/function_advanced.ts\" , \"LookupReturnType\" , & [] , ProjectionMode :: Expanded ,) ; assert_primitive (& expr , PrimitiveName :: Boolean) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "index_signatures.rs",
        row_function: "index_signatures_numeric_index_publishes_signature",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:2347d6eb3abcbddf0a4c08d8191b952e3f1864e94d7ccaf3f3543ebd55e779a4",
        workspace_files: &[("/fixtures/index_signatures.ts", "sha256:fd0225265fe34eec48d929b59302ef3edee0aa21a2990733a84cac5945a79fa0")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/index_signatures.ts\" , \"NumericIndexed\" , & [] , ProjectionMode :: Expanded ,) ; let sigs = object_index_signatures (& expr) ; assert_eq ! (sigs . len () , 1) ; assert_primitive (& sigs [0] . key_type , PrimitiveName :: Number) ; assert_primitive (& sigs [0] . value_type , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "index_signatures.rs",
        row_function: "index_signatures_symbol_index_publishes_signature",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:09d7141d43f903e4c3476bfed7b72f9069f626915843d71d473eded76967b1e0",
        workspace_files: &[("/fixtures/index_signatures.ts", "sha256:fd0225265fe34eec48d929b59302ef3edee0aa21a2990733a84cac5945a79fa0")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/index_signatures.ts\" , \"SymbolIndexed\" , & [] , ProjectionMode :: Expanded ,) ; let sigs = object_index_signatures (& expr) ; assert_eq ! (sigs . len () , 1) ; assert_primitive (& sigs [0] . key_type , PrimitiveName :: Symbol) ; assert_primitive (& sigs [0] . value_type , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "jsx.rs",
        row_function: "jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:63e8ba7568aaa8e1d54664a20ddc06556209a579935b75c6b937ae24554934fd",
        workspace_files: &[("/fixtures/jsx.ts", "sha256:dc9c2d35bed4acdd6551b2f850fb99e45b1e41760e9c6e2946a6fc50cb2db6db")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_jsx (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/jsx.ts\" , \"DivPropsViaIndex\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"className\" , \"id\"]) ; assert_primitive (& props [\"id\"] . ty , PrimitiveName :: String) ; assert_primitive (& props [\"className\"] . ty , PrimitiveName :: String) ; assert ! (props [\"id\"] . optional) ; assert ! (props [\"className\"] . optional) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "jsx.rs",
        row_function: "jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:f5c400f349cacf6b96a2adfe630f928caaa9134b538f8cb6866f357870dc28be",
        workspace_files: &[("/fixtures/jsx.ts", "sha256:dc9c2d35bed4acdd6551b2f850fb99e45b1e41760e9c6e2946a6fc50cb2db6db")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_jsx (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/jsx.ts\" , \"SpanPropsViaIndex\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"title\"]) ; assert_primitive (& props [\"title\"] . ty , PrimitiveName :: String) ; assert ! (props [\"title\"] . optional) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "mapped_modifiers.rs",
        row_function: "mapped_modifier_minus_optional_strips_optional_and_undefined",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:53f7f3d3041b9a920fc879ff726709a2a09393b9a21bb3a25c2a3601d135fb6e",
        workspace_files: &[("/fixtures/mapped_modifiers.ts", "sha256:a82df02cf44225bfe2b470ac7d6ad401eebca75e32a00033fc7855d054314b1b")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/mapped_modifiers.ts\" , \"RemoveOptionalResult\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"a\" , \"b\"]) ; assert ! (! props [\"a\"] . optional) ; assert ! (! props [\"b\"] . optional) ; assert_primitive (& props [\"a\"] . ty , PrimitiveName :: String) ; assert_primitive (& props [\"b\"] . ty , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "mode_boundary_invariants.rs",
        row_function: "mode_boundary_keyof_across_reexport_chain_resolves_all_keys",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:d74afd64677d94ae180282a816ad16b1e9f1306a32364c3d5a77013c6395758a",
        workspace_files: &[("/fixtures/mode_boundary_reexport_barrel.ts", "sha256:08e4a5e89d08c03262dcee73761f65fb932e6183fcfb6cd2cbb53996990c5079"), ("/fixtures/mode_boundary_reexport_leaf.ts", "sha256:8d8a8063d69758e059744fdd12559315386a7d0d5c430028f7a28291f55335c0"), ("/fixtures/mode_boundary_reexport_link_1.ts", "sha256:5d5d80d0df2cd66d6ca7aa96aa898da452c21c42cba58f6cd464caff69ddcc9d"), ("/fixtures/mode_boundary_reexport_link_2.ts", "sha256:e023d2c9fe5a898d709e78af99e1ee2788289181be756956ac129ab3aa149fcb"), ("/fixtures/mode_boundary_reexport_link_3.ts", "sha256:4edcef83077b709e2ba593a1253a9afcf8640c0133c8c9387abcd58f2edd1725"), ("/fixtures/mode_boundary_reexport_link_4.ts", "sha256:21c0a457046ee0671d553e058d37ad74808af60ae3349228b4dccba6d14d4654"), ("/fixtures/mode_boundary_reexport_link_5.ts", "sha256:10350cc32f3b5604e8ca7cfa41d1acfd107c1138e5371c8d40d8e7d4c7da243f"), ("/fixtures/mode_boundary_reexport_link_6.ts", "sha256:e45be5023484d8d774719cd0dd15653cf95a4d945df2d24e125d34d4da9de767"), ("/fixtures/mode_boundary_reexport_principal.ts", "sha256:15dded3cca294e1e4ac9416417c1b2e82c234408ea02b80455618d95b7cdd1f1")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_reexport_chain (& host) ; let (expr , _record) = resolve_with_mode (& host , \"/fixtures/mode_boundary_reexport_principal.ts\" , \"WantedKeys\" , ProjectionMode :: Expanded ,) ; assert_literal_union (& expr , & [\"a\" , \"b\"]) ; }",
    },
    LiftMigrationProvenance {
        row_file: "modern_ts_features.rs",
        row_function: "import_attribute_simulated_string_literal_indexed_member",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:1ca70735186a4f8c30106dae74047648cc404ce9b7bc705325b97f4db5238515",
        workspace_files: &[("/fixtures/modern_ts_features.ts", "sha256:61175abbd39b63c0816ce74fed7c60ccbaa74ef2c8fb6a31ad195d0d6e1617f3")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/modern_ts_features.ts\" , \"ImportedJsonName\" , & [] , ProjectionMode :: Expanded ,) ; assert_string_literal (& expr , \"verter-fixture\") ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "module_features.rs",
        row_function: "module_features_namespace_geometry_vector_aliases_point",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:4ac9755182fdcab6409ea9f35404b44f3e4801e9e57d0db765236e02d6303cdd",
        workspace_files: &[("/fixtures/module_features.ts", "sha256:36970cad08a9bb243edba24691caaaadc19bd8fd8151f502c242b2b47585fbd6")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_main (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/module_features.ts\" , \"GeometryVector\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"x\" , \"y\"]) ; assert_primitive (& props [\"x\"] . ty , PrimitiveName :: Number) ; assert_primitive (& props [\"y\"] . ty , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "module_features.rs",
        row_function: "module_features_typeof_import_default_resolves_value_shape",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:fdc6154808be55e8cc6d029586248485b720fb772ea61f64eb791ac9f5636d3e",
        workspace_files: &[("/fixtures/module_features_base.ts", "sha256:55bb0e735a8370313acb845ea89fb0d3d1553de23bb9e5790c11db62089bb013"), ("/fixtures/module_features_cjs.d.ts", "sha256:cfc702fbbe4e01f53f002eb069b2a8eed0a66121f9fe3aaf0c3c6ef970889300"), ("/fixtures/module_features_consumer.ts", "sha256:550bb0fe8d06c3a8a0a90e6fcd748b49111e48741fabf5e3356d0427dde96cc3"), ("/fixtures/module_features_leaf.ts", "sha256:c5e467e1a5980ceaaac76c5d9fd1ea00cc52b2b76273b67390dbd7851d355699"), ("/fixtures/module_features_patch.ts", "sha256:c6f85a372442fbcfe96970f91f007029a2f03ed273a0abf49bd2528bcde980df")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_consumer_graph (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/module_features_consumer.ts\" , \"LeafDefault\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"count\" , \"tag\"]) ; assert_string_literal (& props [\"tag\"] . ty , \"leaf-default\") ; assert_primitive (& props [\"count\"] . ty , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "module_features.rs",
        row_function: "module_features_typeof_import_named_value_resolves_to_literal",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:10c529bc44dc01641a80c4033861122aea26b76ed178408e773d356f316e26e9",
        workspace_files: &[("/fixtures/module_features_base.ts", "sha256:55bb0e735a8370313acb845ea89fb0d3d1553de23bb9e5790c11db62089bb013"), ("/fixtures/module_features_cjs.d.ts", "sha256:cfc702fbbe4e01f53f002eb069b2a8eed0a66121f9fe3aaf0c3c6ef970889300"), ("/fixtures/module_features_consumer.ts", "sha256:550bb0fe8d06c3a8a0a90e6fcd748b49111e48741fabf5e3356d0427dde96cc3"), ("/fixtures/module_features_leaf.ts", "sha256:c5e467e1a5980ceaaac76c5d9fd1ea00cc52b2b76273b67390dbd7851d355699"), ("/fixtures/module_features_patch.ts", "sha256:c6f85a372442fbcfe96970f91f007029a2f03ed273a0abf49bd2528bcde980df")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_consumer_graph (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/module_features_consumer.ts\" , \"LeafNamedValue\" , & [] , ProjectionMode :: Expanded ,) ; assert_string_literal (& expr , \"leaf\") ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "substitution_types.rs",
        row_function: "substitution_types_sb15_recursive_generic_substitution",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:3f8399fadab2394c803926fa5b0db6dd0411265d3410e694fd5b58b28bbad46e",
        workspace_files: &[("/fixtures/substitution_types.ts", "sha256:41a41aabe8dc03c8de662058d952b7e131b42f59ca0cb23fd4a17f6ef634cb4b")],
        original_body_tokens: "{ let expr = { let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/substitution_types.ts\" , \"Sb15Result\" , & [] , ProjectionMode :: Expanded) ; expr } ; assert_primitive (& expr , PrimitiveName :: Unknown) ; }",
    },
    LiftMigrationProvenance {
        row_file: "typescript_rules.rs",
        row_function: "typescript_rules_awaited_recursively_unwraps_promises",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:c7794986e22f3edbecc33c1f0193d95378ca04a2daec2d395da4c392a08daa02",
        workspace_files: &[("/fixtures/typescript-rules.ts", "sha256:eb2dd8e14722c26d1acd82f1bb056dc844ff2b8823e6b54699372b67cbdc473d")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/typescript-rules.ts\" , TYPESCRIPT_RULES) ; let (expr , record) = resolve_expr (& host , \"/fixtures/typescript-rules.ts\" , \"AwaitedRules\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_boolean_literal (& props [\"done\"] . ty , true) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "typescript_rules.rs",
        row_function: "typescript_rules_constructor_parameters_resolve_tuple",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:75eac635da8a3543e4685df10617d6913fab0d2aa79b5425843467c996445892",
        workspace_files: &[("/fixtures/typescript-rules.ts", "sha256:eb2dd8e14722c26d1acd82f1bb056dc844ff2b8823e6b54699372b67cbdc473d")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/typescript-rules.ts\" , TYPESCRIPT_RULES) ; let (expr , record) = resolve_expr (& host , \"/fixtures/typescript-rules.ts\" , \"ConstructorParamsRules\" , & [] , ProjectionMode :: Expanded ,) ; let TypeExpr :: Tuple { elements , .. } = & expr else { panic ! (\"expected constructor parameter tuple, got {expr:?}\") ; } ; assert_eq ! (elements . len () , 1) ; assert_primitive (& elements [0] . ty , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "typescript_rules.rs",
        row_function: "typescript_rules_indexed_access_reduces_terminal_property",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:be5be6efa5dcacb5a49df0c9c42f6e9998a8705bb8a46cc27b34606e50d4d27e",
        workspace_files: &[("/fixtures/typescript-rules.ts", "sha256:eb2dd8e14722c26d1acd82f1bb056dc844ff2b8823e6b54699372b67cbdc473d")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/typescript-rules.ts\" , TYPESCRIPT_RULES) ; let (expr , record) = resolve_expr (& host , \"/fixtures/typescript-rules.ts\" , \"IndexedRules\" , & [] , ProjectionMode :: Expanded ,) ; assert_primitive (& expr , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "typescript_rules.rs",
        row_function: "typescript_rules_instance_type_resolves_constructed_object",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:bc84f00ac758b7735b0ceec5859378e3309c8b242325f6bceaf00e54031ab9ad",
        workspace_files: &[("/fixtures/typescript-rules.ts", "sha256:eb2dd8e14722c26d1acd82f1bb056dc844ff2b8823e6b54699372b67cbdc473d")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/typescript-rules.ts\" , TYPESCRIPT_RULES) ; let (expr , record) = resolve_expr (& host , \"/fixtures/typescript-rules.ts\" , \"InstanceRules\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_primitive (& props [\"id\"] . ty , PrimitiveName :: String) ; assert_primitive (& props [\"ready\"] . ty , PrimitiveName :: Boolean) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "typescript_rules.rs",
        row_function: "typescript_rules_keyof_materializes_literal_key_union",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:d622e6d404346ac5f2d9f14548ee34a0921db2bf1831bba448db8f6e0e5542d1",
        workspace_files: &[("/fixtures/typescript-rules.ts", "sha256:eb2dd8e14722c26d1acd82f1bb056dc844ff2b8823e6b54699372b67cbdc473d")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/typescript-rules.ts\" , TYPESCRIPT_RULES) ; let (expr , record) = resolve_expr (& host , \"/fixtures/typescript-rules.ts\" , \"KeyOfRules\" , & [] , ProjectionMode :: Expanded ,) ; assert_literal_union (& expr , & [\"count\" , \"id\" , \"nested\"]) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "union_key_access.rs",
        row_function: "union_key_access_keyof_self_projects_full_value_union",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:37da928f7a9edbf849a4f83c3e21fe993ce0cdad9e8408be214056854619acaf",
        workspace_files: &[("/fixtures/union_key_access.ts", "sha256:5314673fda6d33b3431c4c1d493c7729c4c05499d3e73921ab5ac1184b34df3c")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/union_key_access.ts\" , \"EveryMember\" , & [] , ProjectionMode :: Expanded ,) ; assert_union_contains_primitive (& expr , PrimitiveName :: Number) ; assert_union_contains_primitive (& expr , PrimitiveName :: String) ; assert_union_contains_primitive (& expr , PrimitiveName :: Boolean) ; assert_union_contains_primitive (& expr , PrimitiveName :: Null) ; let TypeExpr :: Union (types) = & expr else { panic ! (\"expected union, got {expr:?}\") ; } ; assert_eq ! (types . len () , 4 , \"expected exactly four arms (number, string, boolean, null), got {types:?}\") ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_edge.rs",
        row_function: "utility_edge_non_nullable_strips_null_and_undefined",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:439ac841e1ad252814ff13ae5f8e0d36d60d7169a2a8750f8582da53dc85162e",
        workspace_files: &[("/fixtures/utility_edge.ts", "sha256:94ffcdc97357ffd9df2cac514b68eb96c33219063d789fa509ca846fb1a781b4")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_edge.ts\" , \"NonNullablePrim\" , & [] , ProjectionMode :: Expanded ,) ; assert_primitive (& expr , PrimitiveName :: String) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_edge.rs",
        row_function: "utility_edge_readonly_required_composes_modifiers",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:4f4575b0ee26abd7d94d272ddb78996d18ca3f742e542ec4376e5645d79acbdd",
        workspace_files: &[("/fixtures/utility_edge.ts", "sha256:94ffcdc97357ffd9df2cac514b68eb96c33219063d789fa509ca846fb1a781b4")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_edge.ts\" , \"ReadonlyRequiredOptional\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"a\" , \"b\"]) ; assert ! (! props [\"a\"] . optional) ; assert ! (! props [\"b\"] . optional) ; assert ! (props [\"a\"] . readonly) ; assert ! (props [\"b\"] . readonly) ; assert_primitive (& props [\"a\"] . ty , PrimitiveName :: String) ; assert_primitive (& props [\"b\"] . ty , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_edge.rs",
        row_function: "utility_edge_required_strips_optional_markers",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:6074c878caba92c8a918fec231b8ea0cc3cc842341ec9722b94009f7da5d4a56",
        workspace_files: &[("/fixtures/utility_edge.ts", "sha256:94ffcdc97357ffd9df2cac514b68eb96c33219063d789fa509ca846fb1a781b4")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_edge.ts\" , \"RequiredOptional\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"a\" , \"b\"]) ; assert ! (! props [\"a\"] . optional) ; assert ! (! props [\"b\"] . optional) ; assert_primitive (& props [\"a\"] . ty , PrimitiveName :: String) ; assert_primitive (& props [\"b\"] . ty , PrimitiveName :: Number) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb15_awaited_unknown_is_unknown",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:63a8c43afcad26f33014ce928756e4589bc1e12c5511413fa1155e760f1b89c6",
        workspace_files: &[("/fixtures/utility_top_bottom.ts", "sha256:878b32c48b8f73c6cb87a334d7f9836250e206087077e10a1984fc2d36c0cc9b")],
        original_body_tokens: "{ let expr = { let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_top_bottom.ts\" , \"Utb15AwaitedUnknown\" , & [] , ProjectionMode :: Expanded) ; expr } ; assert_primitive (& expr , PrimitiveName :: Unknown) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb17_awaited_null_is_null",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:6578a797ff2fa24abf92dd30ff37a868ba90baed6c57b283ac6a06bad3557684",
        workspace_files: &[("/fixtures/utility_top_bottom.ts", "sha256:878b32c48b8f73c6cb87a334d7f9836250e206087077e10a1984fc2d36c0cc9b")],
        original_body_tokens: "{ let expr = { let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_top_bottom.ts\" , \"Utb17AwaitedNull\" , & [] , ProjectionMode :: Expanded) ; expr } ; assert_primitive (& expr , PrimitiveName :: Null) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb18_awaited_undefined_is_undefined",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:138f6a72585d54f948baec63b99976a1ca8252b2bd26c3d6bad4ea4561bbf4cf",
        workspace_files: &[("/fixtures/utility_top_bottom.ts", "sha256:878b32c48b8f73c6cb87a334d7f9836250e206087077e10a1984fc2d36c0cc9b")],
        original_body_tokens: "{ let expr = { let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_top_bottom.ts\" , \"Utb18AwaitedUndefined\" , & [] , ProjectionMode :: Expanded) ; expr } ; assert_primitive (& expr , PrimitiveName :: Undefined) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb19_awaited_nested_promise_is_inner_primitive",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:131e6401a417beb7b78dd08290bfa5f53d147f74ad09c401c185295507e68855",
        workspace_files: &[("/fixtures/utility_top_bottom.ts", "sha256:878b32c48b8f73c6cb87a334d7f9836250e206087077e10a1984fc2d36c0cc9b")],
        original_body_tokens: "{ let expr = { let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_top_bottom.ts\" , \"Utb19AwaitedNestedPromise\" , & [] , ProjectionMode :: Expanded) ; expr } ; assert_primitive (& expr , PrimitiveName :: String) ; }",
    },
    LiftMigrationProvenance {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb21_non_nullable_unknown_is_empty_object",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:344020e2fe913b4fd7e5059f2923ef03763b36be24a826efafb9845161bc112a",
        workspace_files: &[("/fixtures/utility_top_bottom.ts", "sha256:878b32c48b8f73c6cb87a334d7f9836250e206087077e10a1984fc2d36c0cc9b")],
        original_body_tokens: "{ let expr = { let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/utility_top_bottom.ts\" , \"Utb21NonNullableUnknown\" , & [] , ProjectionMode :: Expanded) ; expr } ; let TypeExpr :: Object (object) = & expr else { panic ! (\"expected empty object, got {expr:?}\") ; } ; assert ! (object . properties . is_empty () , \"expected empty `{{}}`, got {:?}\" , object . properties ,) ; }",
    },
    LiftMigrationProvenance {
        row_file: "variadic_tuples.rs",
        row_function: "variadic_tuple_concat_alias_produces_joined_literal_tuple",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:b8fdb902a4036d6a9071058659fc8b2a6b1452a274d25524dd9775308202dd45",
        workspace_files: &[("/fixtures/variadic_tuples.ts", "sha256:61e329aed5b8d2c079d67ee2fc2b39fd8f9abb8efb5c6ebb03a0433a8c37b0b4")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/variadic_tuples.ts\" , \"ConcatPair\" , & [] , ProjectionMode :: Expanded ,) ; let TypeExpr :: Tuple { elements , readonly } = & expr else { panic ! (\"expected tuple, got {expr:?}\") ; } ; assert ! (! readonly) ; assert_eq ! (elements . len () , 4) ; assert_number_literal (& elements [0] . ty , 1.0) ; assert_number_literal (& elements [1] . ty , 2.0) ; assert_number_literal (& elements [2] . ty , 3.0) ; assert_number_literal (& elements [3] . ty , 4.0) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "wide_deep.rs",
        row_function: "wide_deep_projected_token_resolves_literal_union",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:844f65b203f46bf5c7876121295c75b5513ee785180a5841ecac9f64e7011d39",
        workspace_files: &[("/fixtures/wide-deep.ts", "sha256:7cfb3d614dbc6ea427f24882e6bb7c40bf2ed4042287c39aee3192ec6337fab9")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/wide-deep.ts\" , WIDE_DEEP) ; let (expr , record) = resolve_expr (& host , \"/fixtures/wide-deep.ts\" , \"WideDeepProjectedToken\" , & [] , ProjectionMode :: Expanded ,) ; assert_literal_union (& expr , & [\"alpha\" , \"beta\" , \"gamma\"]) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    // U2.MAPPED_TEMPLATE-era lifts (tsgo-available oracle lift). Provenance
    // emitted by the audited `emit_lifted_row_migrations` lift-capture (the
    // closed `syn` extractor over the original `#[ignore]` body); the
    // body-extracted fingerprint matched the registry-projected fingerprint, so
    // the registry faithfully reproduces each original query.
    LiftMigrationProvenance {
        row_file: "mapped_template.rs",
        row_function: "record_with_template_literal_key_union_projects_root_slot",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:f8c259acdc518799ba6dfdbf6598ac3a9e3fac3dab7c265d22d4d0ef9d3c1d47",
        workspace_files: &[("/fixtures/mapped-template.ts", "sha256:68b1c5433dd696540382c2d22f1a491e6bc355c9a73652eb6bc7ebc8923c65cf")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert_ts (& host , \"/fixtures/mapped-template.ts\" , MAPPED_TEMPLATE) ; let (expr , record) = resolve_expr (& host , \"/fixtures/mapped-template.ts\" , \"RecordTemplateRootSlot\" , & [] , ProjectionMode :: Expanded ,) ; let slot = function_type (& expr) ; assert_eq ! (slot . parameters . len () , 1) ; let payload = object_props (& slot . parameters [0] . ty) ; assert_literal_union (& payload [\"name\"] . ty , & [\"item\" , \"root\"]) ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
    LiftMigrationProvenance {
        row_file: "template_literal_inference.rs",
        row_function: "template_literal_key_remap_capitalises_each_event_key",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint: "blake3:4376ec8e27b02314d0af94a7d6de18c2a6a468d1090dc236b2216096c43305bc",
        workspace_files: &[("/fixtures/template_literal_inference.ts", "sha256:383c4fdd04efadb1f7344d3b8518313172211f86e2e712e570e82d25d8b8fb44")],
        original_body_tokens: "{ let host = make_host_with_footprint () ; upsert (& host) ; let (expr , record) = resolve_expr (& host , \"/fixtures/template_literal_inference.ts\" , \"CounterHandlers\" , & [] , ProjectionMode :: Expanded ,) ; let props = object_props (& expr) ; assert_eq ! (prop_names (& props) , vec ! [\"onDec\" , \"onInc\"]) ; let on_inc = function_type (& props [\"onInc\"] . ty) ; assert_eq ! (on_inc . parameters . len () , 1) ; assert_string_literal (& on_inc . parameters [0] . ty , \"inc\") ; let on_dec = function_type (& props [\"onDec\"] . ty) ; assert_eq ! (on_dec . parameters . len () , 1) ; assert_string_literal (& on_dec . parameters [0] . ty , \"dec\") ; assert_query_mode (& record , ProjectionModeTag :: Expanded) ; }",
    },
];
