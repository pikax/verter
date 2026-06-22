//! Definition / type-definition merge plus carrier-path normalization.
//!
//! Resolves TypeProvider definition targets back to carrier-source positions:
//! a carrier IDE virtual file maps through its own CodeTransform sourcemap (the
//! in-context mapper for the queried file, the external resolver for a foreign
//! component); every other target reads its own source through the host VFS and
//! converts the byte offsets, fail-closed. Also owns the `{carrier}` virtual-file
//! path classification/normalization helpers shared with the other merge paths.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::type_provider::protocol::TypeLocation;
use crate::uri::path_to_file_uri;

use super::position::{
    tsx_range_to_carrier_range, BarrelResolver, ExternalIdeResolver, ExternalSourceReader,
};

/// Resolve a definition/type-definition target's byte-offset range to an LSP `Range` by
/// reading the target's own source through the host workspace (VFS) and converting through
/// [`LineIndex`].
///
/// The definition/type-definition providers produce `start`/`end` as REAL byte offsets into
/// the target file. `read_source` hands back that same source — routed through
/// `verter_workspace::WorkspaceRead::read_file` (host cache → snapshot → disk), so a cold
/// target is read and cached once and an open editor buffer's overlay wins over stale disk
/// content — and the offsets convert to line:col in the client-negotiated encoding. It is only
/// called when the emitted URI is the very file those offsets index (path normalization was a
/// no-op), so the offsets are valid.
///
/// The source read is the workspace layer's job, never `std::fs`: the VFS is the single
/// source-read authority for the LSP, which is exactly what `no_std_fs_in_semantic_session_paths`
/// enforces over this crate.
///
/// Returns `None` (FAIL-CLOSED) when the source cannot be read or an offset falls outside it.
/// Callers MUST then drop the location — never substitute `Range::default()`, which silently
/// sends the editor to line 0 of the wrong place.
///
/// This resolves an external definition/type-definition range from the target's own on-disk
/// source: the provider already read this file to compute the offsets, and this re-reads it
/// (through the VFS) to convert those byte offsets back to a line:col `Range`. The resolver
/// covers definition/type-definition only, where the offsets are guaranteed to index the
/// target's own source.
pub(crate) fn resolve_external_target_range(
    path: &str,
    start: u32,
    end: u32,
    encoding: PositionEncodingKind,
    read_source: ExternalSourceReader<'_>,
) -> Option<Range> {
    let source = read_source(path)?;
    let line_index = LineIndex::new(&source, encoding);
    Some(Range {
        start: line_index.offset_to_position(start)?,
        end: line_index.offset_to_position(end)?,
    })
}

/// Resolve a carrier IDE (`{carrier}.tsx`/`.jsx`) target's byte offsets to a carrier-source
/// [`Range`], FAIL-CLOSED. The single strict carrier-IDE mapper shared by every edit/navigation
/// merge (definition, type-definition, references, rename, code actions).
///
/// The provider's offsets index a generated IDE TSX file; mapping them back to the carrier
/// source requires THAT file's own CodeTransform sourcemap, so the resolver is split by
/// whether the target is the file currently being queried:
///
/// - **Current provider file** (`path` is the SAME canonical file as `current_tsx_path`): the
///   in-context `mapper` / line indexes passed by the handler already describe this exact TSX,
///   so map through them. Identity is decided by the single canonical path owner
///   ([`verter_span::path::canonicalize_path_cow`]) — never a raw `==`, which would treat the
///   same file spelled differently (backslashes vs forward slashes, drive-letter case, a
///   `\\?\` extended prefix) as foreign and false-drop a genuine same-file edit.
/// - **Foreign carrier IDE file** (another component's generated file): only the external
///   resolver can supply the correct mapper. The current file's mapper describes a *different*
///   file, so reusing it would land on the wrong token — and the old `.unwrap_or_default()`
///   collapsed a failed reuse into a line-0 range pointing into the wrong file. There is
///   deliberately NO current-mapper fallback for foreign targets.
///
/// The provider mapper is the projection-agnostic [`ProviderPositionMapper`]: its `SourceMap`
/// variant preserves the strict source-map run semantics 1:1 for `{carrier}.tsx`, while its
/// `SelfFile` variant (a `.svelte.ts` rune module) needs no separate range algorithm —
/// [`tsx_range_to_carrier_range`] delegates through the enum's `tsx_range_to_carrier` and any
/// synthetic / prelude / unmapped region returns `None` (fail-closed preserved).
///
/// Returns `None` whenever the correct sourcemap is unavailable (no/unknown external resolver)
/// or the offsets do not map. The caller MUST drop the location/edit — never substitute
/// `Range::default()`, which silently sends the editor to line 0, or for a rename WRITES the
/// new name at line 0 of the wrong file and corrupts it.
#[expect(
    clippy::too_many_arguments,
    reason = "current-file context (path + indexes + mapper) plus the foreign-file resolver"
)]
pub(crate) fn resolve_carrier_ide_range_strict(
    path: &str,
    start: u32,
    end: u32,
    current_tsx_path: &str,
    current_tsx_line_index: &LineIndex,
    current_mapper: &ProviderPositionMapper,
    current_carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
) -> Option<Range> {
    // Same-file identity via the single canonical path owner (backslash→slash, drive-letter
    // case, `\\?\` extended prefix all folded) — NEVER a raw `==`.
    if verter_span::path::canonicalize_path_cow(path)
        == verter_span::path::canonicalize_path_cow(current_tsx_path)
    {
        return tsx_range_to_carrier_range(
            start,
            end,
            current_tsx_line_index,
            current_mapper,
            current_carrier_line_index,
        );
    }

    // Foreign generated TSX: the in-context mapper describes a different file. Only its own
    // sourcemap (via the external resolver) can map the offsets — fail closed otherwise.
    let resolver = external_resolver?;
    let ctx = resolver(path)?;
    // A foreign `addMissingImport` insertion sits in the foreign TSX's synthetic helper-import
    // preamble. The foreign source map can STRICT-MAP that offset to the foreign carrier `(0,0)`
    // (the foreign file top, ABOVE its `<script setup>`) — the same geometry that makes the
    // current-file preamble bug real. A foreign preamble insertion is never placeable from its
    // strict range, and there is NO foreign `<script setup>` anchor in this merge (the re-anchor
    // describes the CURRENT request only), so it must DROP. Two structural discriminators, mirroring
    // the current-file code-action guard (`feature_merges::merge_code_actions`):
    //
    // 1. The foreign context publishes the typed `x_verter_helper_preamble_end` boundary ⇒ classify
    //    via [`is_preamble_import_insertion`] (zero-width geometry at/before the boundary) and drop.
    // 2. The foreign map carries NO boundary AND the edit is zero-width ⇒ fail closed. A real Verter
    //    foreign carrier-IDE projection always publishes the boundary, so its absence is a stale /
    //    non-Verter artifact; a zero-width insertion that strict-maps to the foreign `(0,0)` would
    //    otherwise be accepted at the foreign file top. The classifier returns `false` with no
    //    boundary (it cannot prove the preamble), so this explicit absent-boundary drop is required.
    //
    // Both are STRUCTURE only (geometry + the foreign boundary), never `new_text` and never the
    // produced `(0,0)` value. A non-zero-width foreign edit (a rename replacement, a remove-unused
    // deletion, an ordinary replacement) is unaffected by either branch and keeps its strict path.
    if crate::type_provider::auto_import::is_preamble_import_insertion(
        start,
        end,
        &ctx.tsx_line_index,
        &ctx.mapper,
    ) {
        return None;
    }
    if start == end && ctx.mapper.helper_preamble_end().is_none() {
        return None;
    }
    tsx_range_to_carrier_range(
        start,
        end,
        &ctx.tsx_line_index,
        &ctx.mapper,
        &ctx.carrier_line_index,
    )
}

/// Merge verter definition with TypeProvider definitions.
///
/// Strategy:
/// - If verter provides a definition, use it (it's already precise for in-file navigation)
/// - TypeProvider definitions are used for cross-file navigation (import targets, etc.)
/// - Map TypeProvider locations back to Vue positions where applicable
///
/// `external_resolver` is used to resolve positions in `.vue.tsx` files that differ
/// from the current file (cross-file navigation, e.g., CTRL+CLICK on component tag
/// navigates to the target component's file).
#[expect(
    clippy::too_many_arguments,
    reason = "definition merging needs mapper, indexes, URI, and resolver inputs"
)]
pub fn merge_definitions(
    verter_def: Option<GotoDefinitionResponse>,
    type_defs: Vec<TypeLocation>,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    document_uri: &Uri,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Option<GotoDefinitionResponse> {
    merge_definitions_with_barrel_resolver(
        verter_def,
        type_defs,
        current_tsx_path,
        tsx_line_index,
        mapper,
        carrier_line_index,
        external_resolver,
        document_uri,
        carrier_source_exists,
        None,
        negotiated_encoding,
        source_reader,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "barrel-aware definition merging adds one resolver to the shared merge context"
)]
pub fn merge_definitions_with_barrel_resolver(
    verter_def: Option<GotoDefinitionResponse>,
    type_defs: Vec<TypeLocation>,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    document_uri: &Uri,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    barrel_resolver: Option<BarrelResolver<'_>>,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Option<GotoDefinitionResponse> {
    // If verter provides a definition, prefer it when:
    // - TSGO returned nothing, or
    // - verter resolved cross-file (TSGO often returns *.vue shim declarations)
    //
    // Compare against the actual document URI — the sentinel has already been
    // replaced by the server handler before this function is called.
    if let Some(ref vd) = verter_def {
        let is_cross_file = matches!(vd, GotoDefinitionResponse::Scalar(loc)
            if loc.uri != *document_uri
            && loc.uri.as_str() != crate::features::definition::SAME_FILE_URI_STR);
        let is_same_file =
            matches!(vd, GotoDefinitionResponse::Scalar(loc) if loc.uri == *document_uri);
        if type_defs.is_empty() || is_cross_file || is_same_file {
            return verter_def;
        }
    }

    // If TypeProvider provides definitions, convert them
    if !type_defs.is_empty() {
        let mut locations: Vec<Location> = type_defs
            .into_iter()
            .filter_map(|loc| {
                // A carrier IDE virtual file (`{carrier}.tsx`/`.jsx`): the provider's byte
                // offsets index the generated TSX; map them back to the carrier source through
                // that file's own CodeTransform sourcemap — the current file's in-context mapper
                // for the file being queried, the external resolver for a foreign component.
                // Fail closed (drop the location) when no sourcemap bridges the offsets; never
                // collapse to a line-0 range pointing into the wrong file. Generalized to the
                // registry carrier-extension set so `.svelte` carriers get the same fix.
                if is_carrier_ide_path(&loc.path) {
                    let uri =
                        path_to_uri(normalize_carrier_path(&loc.path, carrier_source_exists))?;
                    let range = resolve_carrier_ide_range_strict(
                        &loc.path,
                        loc.start,
                        loc.end,
                        current_tsx_path,
                        tsx_line_index,
                        mapper,
                        carrier_line_index,
                        external_resolver,
                    )?;
                    return Some(Location { uri, range });
                }

                // Every other target emits the normalized path's URI. When normalization
                // is a no-op the emitted URI IS the file the provider's byte offsets index
                // (`.d.ts`/`.ts`/`.js`/…, or a real `{carrier}.ts` with no backing carrier
                // source), so read that source and convert the offsets to a real `Range`.
                // Barrel re-exports (terminal-decl follow) take priority for those real files;
                // fail closed when the source can't be read — never collapse to line 0.
                let normalized = normalize_carrier_path(&loc.path, carrier_source_exists);
                if normalized == loc.path {
                    if let Some(resolver) = barrel_resolver {
                        if let Some(location) = resolver(&loc.path, loc.start, loc.end) {
                            return Some(location);
                        }
                    }
                    let uri = path_to_uri(normalized)?;
                    let range = resolve_external_target_range(
                        &loc.path,
                        loc.start,
                        loc.end,
                        negotiated_encoding.clone(),
                        source_reader,
                    )?;
                    return Some(Location { uri, range });
                }

                // Normalization rewrote the path (`{carrier}.d.ts`/`{carrier}.ts` → carrier
                // source, or another file's `{carrier}.tsx` → carrier source): the offsets index
                // the generated declaration file, but the URI we emit is the carrier source and
                // no in-context sourcemap bridges them. Fail closed rather than send the editor
                // to line 0 of the wrong file.
                None
            })
            .collect();

        if locations.is_empty() {
            return verter_def;
        }

        // Deduplicate by (uri, range): distinct definitions in the same file (e.g. two
        // overloads in one `.d.ts`) must survive, while spans that resolve to the exact
        // same location collapse.
        let mut seen = std::collections::HashSet::new();
        locations.retain(|loc| {
            seen.insert((
                loc.uri.clone(),
                loc.range.start.line,
                loc.range.start.character,
                loc.range.end.line,
                loc.range.end.character,
            ))
        });

        // Prefer non-carrier definitions over carrier re-export sites.
        // When CTRL+CLICKing a library symbol (e.g., `onClickOutside` from @vueuse/core),
        // TSGO may return both the real definition (.d.mts) and carrier consumer files.
        let has_non_carrier = locations
            .iter()
            .any(|l| !verter_workspace::path_is_carrier(l.uri.as_str()));
        if has_non_carrier {
            locations.retain(|l| !verter_workspace::path_is_carrier(l.uri.as_str()));
        }

        return Some(if locations.len() == 1 {
            GotoDefinitionResponse::Scalar(locations.into_iter().next().unwrap())
        } else {
            GotoDefinitionResponse::Array(locations)
        });
    }

    verter_def
}

/// Normalize a TypeProvider path back to the original Vue file path.
///
/// Strips virtual file suffixes from Verter-generated paths:
/// - `.vue.tsx` / `.vue.jsx` → `.vue` (IDE output)
/// - `.vue.ts` → `.vue` (public API / DTS output)
/// - `.vue.d.ts` → `.vue` (published type declarations)
///
/// The `carrier_source_exists` predicate guards against collisions with real
/// `.vue.tsx`/`.vue.ts` files on disk: if the backing `.vue` source does
/// not exist in the host, the path is left unchanged. The `.vue.d.ts`
/// case (from node_modules) has no collision risk and skips the check.
pub(crate) fn normalize_carrier_path<'a>(
    path: &'a str,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> &'a str {
    // The IDE virtual file is `{carrier}.tsx`/`.jsx`; stripping the trailing
    // `.tsx`/`.jsx` yields a carrier path (`Foo.vue.tsx` → `Foo.vue`,
    // `Bar.svelte.tsx` → `Bar.svelte`). The carrier-extension set is the
    // registry's (`path_is_carrier`), not a `.vue` literal.
    if (path.ends_with(".tsx") || path.ends_with(".jsx"))
        && verter_workspace::path_is_carrier(&path[..path.len() - 4])
    {
        let candidate = &path[..path.len() - 4]; // strip .tsx/.jsx
        if carrier_source_exists(candidate) {
            return candidate;
        }
    } else if path.ends_with(".d.ts") && verter_workspace::path_is_carrier(&path[..path.len() - 5])
    {
        // The `{carrier}.d.ts` accepted-spelling alias — from node_modules, no
        // collision risk.
        return &path[..path.len() - 5];
    } else if path.ends_with(".ts") && verter_workspace::path_is_carrier(&path[..path.len() - 3]) {
        let candidate = &path[..path.len() - 3]; // strip .ts
        if carrier_source_exists(candidate) {
            return candidate;
        }
    }
    path
}

/// Whether `path` is a carrier IDE virtual file (`{carrier}.tsx` / `.jsx`) —
/// the TSGO IDE output that maps back to a carrier source through the source
/// map. Generalized to the registry carrier-extension set (Vue + Svelte).
pub(crate) fn is_carrier_ide_path(path: &str) -> bool {
    (path.ends_with(".tsx") || path.ends_with(".jsx"))
        && verter_workspace::path_is_carrier(&path[..path.len() - 4])
}

/// Whether `path` is a carrier PUBLIC-API virtual file (`{carrier}.ts`, e.g.
/// `MyComp.vue.ts`) backed by a real `.vue`/`.svelte` source — the macro-derived
/// declaration surface synced for cross-file type resolution. Its byte offsets
/// index the generated API surface and map back to the carrier source through
/// that surface's own CodeTransform sourcemap (NOT the `.tsx` IDE sourcemap).
///
/// The `carrier_source_exists` guard mirrors [`normalize_carrier_path`]'s `.ts`
/// arm: a real on-disk `{name}.{carrier}.ts` with no backing carrier source (or a
/// plain `.ts`) is NOT an API surface and must not be claimed here.
pub(crate) fn is_carrier_api_path(
    path: &str,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> bool {
    // Exclude `.d.ts` (handled by its own normalization arm) before the `.ts` test.
    if path.ends_with(".d.ts") {
        return false;
    }
    path.ends_with(".ts")
        && verter_workspace::path_is_carrier(&path[..path.len() - 3])
        && carrier_source_exists(&path[..path.len() - 3])
}

/// Like `normalize_carrier_path` but returns an owned String.
/// Used by server.rs for inline path normalization.
pub fn normalize_carrier_path_owned(
    path: &str,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> String {
    normalize_carrier_path(path, carrier_source_exists).to_string()
}

/// Convert a file path to a `file://` URI.
///
/// Handles both Windows (`C:/Users/...`) and Unix (`/home/user/...`) paths.
/// Also available as `file_path_to_uri` for use outside this module.
pub fn file_path_to_uri(path: &str) -> Option<Uri> {
    path_to_uri(path)
}

/// Convert a file path to a `file://` URI (internal).
pub(crate) fn path_to_uri(path: &str) -> Option<Uri> {
    path_to_file_uri(path)
}
