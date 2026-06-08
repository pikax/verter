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
// The registry seats the 4 lifted rows (the two index-signature
// publication queries + the two built-in modifier-utility queries). The types +
// validation are additionally exercised with synthetic specs by the
// discriminating guards.

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

/// The closed registry table. Holds the lifted rows — the two index-signature
/// publication queries and the two built-in modifier-utility queries
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
