//! Cross-file `<Child prop=…>` rename resolution.
//!
//! Inherent-impl extension methods on [`super::VerterLanguageServer`] plus the
//! shared classification types, covering: the SHARED prop-usage resolution
//! ([`VerterLanguageServer::resolve_child_prop_usage_at_cursor`], also consumed by
//! the goto-definition props branch in `component_resolve`), inline `defineProps`
//! macro-field declaration resolution, the imported-type
//! (`defineProps<ImportedType>()`) declaration `get_definition` UPGRADE hop, and the
//! [`ChildPropRenameClass`] the merged-edit completeness gate consumes.
//!
//! Split out of `component_resolve` along the rename-feature boundary: the goto /
//! hover / barrel-resolution methods stay there; this module owns only the cross-file
//! rename resolution unit.

use tower_lsp_server::ls_types::{GotoDefinitionResponse, Location, Position, Range, Uri};

use crate::documents::line_index::LineIndex;
use crate::documents::uri_to_canonical_id;
use crate::type_provider::merge;

use super::server_utils::{attr_name_match_rank, location_from_span, to_pascal_case};
use super::{ResolvedComponentDocument, TypeProviderContext, VerterLanguageServer};

/// ALL mapped [`Location`]s of a [`GotoDefinitionResponse`] — the full candidate
/// set the imported-type declaration hop considers (NOT just the first). A
/// definition merge can map a single provider hop to several candidates (e.g. the
/// usage occurrence plus the real member); the declaration selector must inspect
/// every one so a leading non-declaration candidate does not discard a later valid
/// declaration.
fn all_locations_of(response: Option<GotoDefinitionResponse>) -> Vec<Location> {
    match response {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
    }
}

/// Select the single validated DECLARATION `{uri, range}` from the full set of
/// mapped definition candidates — the in-layer declaration proof for the
/// imported-type rename hop. Pure over its inputs (the spelling check is injected),
/// so it is unit-testable with synthetic candidate sets.
///
/// `get_definition` is a declaration query by the LSP contract, so the mapped
/// candidates are declaration-shaped; this function adds the bounded, no-resolver
/// hardening on top of that contract:
/// - Considers ALL `mapped_locations` (never just the first), so a leading
///   non-declaration candidate (e.g. the parent's own usage occurrence a
///   project-membership-limited provider can resolve to) does not discard a later
///   genuine declaration.
/// - Accepts a candidate ONLY when (a) its resolved range spells EXACTLY the prop
///   name (`spells_prop_name`, the name-equality VALIDATION tripwire) AND (b) it is
///   a DISTINCT location from the initiating usage — a candidate equal to ANY
///   `parent_usage_ranges` entry (same `uri` and `range`) is the usage itself, never
///   a declaration, and is rejected.
///
/// Returns the first candidate that passes BOTH gates, or `None` (fail closed) when
/// no candidate is an accepted declaration — the gate then ships no usage-only
/// partial.
fn select_validated_declaration(
    mapped_locations: Vec<Location>,
    parent_usage_ranges: &[(Uri, Range)],
    prop_name: &str,
    spells_prop_name: impl Fn(&Uri, Range, &str) -> bool,
) -> Option<(Uri, Range)> {
    mapped_locations.into_iter().find_map(|location| {
        // REJECT a candidate that coincides with the INITIATING parent usage: a
        // provider whose cross-file project membership cannot reach the imported
        // member resolves `get_definition` back to the prop's OWN usage occurrence —
        // that is NOT a declaration. A declaration must be a DISTINCT location from
        // the usage; accepting a usage location would make the gate's declaration-leg
        // and usage-leg checks pass on the SAME edit, re-opening the usage-only-partial
        // leak.
        let is_parent_usage = parent_usage_ranges.iter().any(|(usage_uri, usage_range)| {
            location.uri == *usage_uri && location.range == *usage_range
        });
        if is_parent_usage {
            return None;
        }

        // TRIPWIRE: the resolved source range must spell EXACTLY the prop name — a
        // name-equality VALIDATION of the structurally-resolved range, never a text
        // search that drives the result. Fail closed on mismatch.
        if spells_prop_name(&location.uri, location.range, prop_name) {
            Some((location.uri, location.range))
        } else {
            None
        }
    })
}

/// A `<Child prop=…>` prop-usage hit resolved at the cursor: the parent-side
/// usage facts plus the resolved child component identity. The SHARED resolution
/// both goto-definition (the props branch of
/// [`VerterLanguageServer::try_component_contract_definition`]) and cross-file
/// rename ([`VerterLanguageServer::classify_child_prop_rename`]) build on, so the
/// two paths cannot drift on the component match, the prop-name-span containment,
/// or the child resolution.
pub(super) struct ChildPropUsage {
    /// The parent SFC's URI (where the `<Child prop=…>` usage lives).
    pub(super) parent_uri: Uri,
    /// The prop name at the cursor.
    pub(super) parent_prop_name: String,
    /// The `.vue` byte span of just the prop NAME on the usage.
    pub(super) parent_prop_name_span: verter_span::Span,
    /// Whether the usage is a same-name shorthand (`:bar` with no expression).
    pub(super) parent_is_shorthand: bool,
    /// The child component's `{carrier}.ts` PUBLIC-API provider path (the surface
    /// the INLINE cross-file prop rename's declaration leg is synthesized against).
    pub(super) child_carrier_api_path: String,
}

/// A resolved [`ChildPropUsage`] together with the resolved child component
/// document (its analysis + line index), so both consumers can read the child's
/// `defineProps` macro fields / template prop definitions without re-resolving.
pub(super) struct ResolvedChildPropUsage {
    pub(super) usage: ChildPropUsage,
    pub(super) child: ResolvedComponentDocument,
}

/// The outcome of [`VerterLanguageServer::resolve_child_prop_usage_at_cursor`].
pub(super) enum ChildPropUsageClass {
    /// The cursor is not on a child component's prop NAME.
    NotChildProp,
    /// The cursor is on a `<Child prop=…>` prop name and the child component
    /// resolved. Boxed — the resolved payload (a full child analysis snapshot) is
    /// much larger than the empty `NotChildProp` variant.
    Resolved(Box<ResolvedChildPropUsage>),
}

/// The resolved DECLARATION target a confirmed `<Child prop=…>` rename must edit
/// — ONE completeness policy with two proof strengths. A rename is complete only
/// when the merged `WorkspaceEdit` edits BOTH the parent usage AND this declaration
/// target.
///
/// The inline-vs-imported distinction is ONLY about how the declaration target is
/// RESOLVED, never two separate policies: both produce a `Known { uri, range }` the
/// completeness gate validates identically. An unresolved declaration is
/// [`ChildPropDeclarationProof::Unknown`] — the rename then fails closed (never
/// ships a usage-only partial).
pub(super) enum ChildPropDeclarationProof {
    /// The prop's declaration identity resolved to `uri` at `range` (negotiated
    /// encoding). Resolved from EITHER the child's inline `defineProps` macro field
    /// span (the child `.vue`) OR a `defineProps<ImportedType>()` imported-type
    /// member declaration in a THIRD file. The completeness gate requires the merged
    /// `WorkspaceEdit` to edit exactly this full range with `new_text == new_name`.
    Known {
        /// The declaration file's URI.
        uri: Uri,
        /// The declaration's full [`Range`] in the negotiated encoding — the EXACT
        /// span the completeness gate proves the merged edit covers. `None` when the
        /// originating span did not resolve to a position (the gate then has no
        /// precise range to assert and fails closed).
        range: Option<Range>,
        /// The INLINE-only synthesis seam: when the declaration came from the
        /// child's `defineProps` MACRO field (not an imported-type member), this is
        /// the prop's file-absolute child `.vue` declaration span the API generator
        /// keys its source-map token on, so Verter can SYNTHESIZE the child-`.vue`
        /// rename leg the provider may not enumerate (tsgo does not). `None` for the
        /// imported-type member case (the provider's own native rename edits the
        /// third file; there is no inline child-`.vue` member to synthesize).
        inline_decl_span: Option<verter_span::Span>,
    },
    /// The declaration identity could NOT be resolved — neither an inline macro
    /// field nor a resolvable imported-type member. The completeness gate fails
    /// closed (returns no edit) rather than ship a usage-only partial. Tracked
    /// follow-up: broaden declaration resolution for more-complex type constructs;
    /// until then an unresolved confirmed child-prop rename MUST fail closed.
    Unknown,
}

/// A CONFIRMED `<Child prop=…>` cross-file rename: the parent-side usage facts plus
/// the resolved declaration proof. The completeness gate requires the merged
/// `WorkspaceEdit` to edit BOTH the parent usage AND the declaration.
pub(super) struct ConfirmedChildPropRename {
    /// The parent-side usage facts (parent URI, prop name + name span, the child's
    /// `.vue` URI and `{carrier}.ts` PUBLIC-API path).
    pub(super) usage: ChildPropUsage,
    /// The parent `.vue` prop-usage NAME [`Range`] in the NEGOTIATED encoding — the
    /// initiating-edit position. The completeness gate proves the merged
    /// `WorkspaceEdit` edits the parent usage (a declaration-only result is also
    /// incomplete). `None` when the usage name span does not resolve to a parent
    /// `.vue` position (the gate then fails closed).
    pub(super) expected_parent_usage_range: Option<Range>,
    /// The resolved declaration target — the single completeness policy with proof
    /// strengths (`Known` / `Unknown`).
    pub(super) declaration: ChildPropDeclarationProof,
}

/// The classification of a rename position with respect to the cross-file
/// `<Child prop=…>` rename — a 2-state result, NOT a lossy `Option`, so the caller
/// distinguishes "not a child prop" (fall through to the provider result untouched)
/// from "a confirmed child prop" (the completeness gate applies, on EVERY return
/// path, and fails closed when the declaration is unresolved or the merged edit is
/// incomplete).
pub(super) enum ChildPropRenameClass {
    /// The cursor is not on a child component's prop name → not a cross-file
    /// child-prop rename. The provider's own result is used as-is (no gate).
    NotChildProp,
    /// The cursor IS on a child component's prop name → a confirmed cross-file
    /// child-prop rename. The completeness gate applies. Boxed — the payload carries
    /// usage facts much larger than the empty `NotChildProp` variant.
    Confirmed(Box<ConfirmedChildPropRename>),
}

impl VerterLanguageServer {
    /// SHARED `<Child prop=…>` prop-usage resolution: walk the template
    /// components, find the usage whose prop NAME span contains the cursor, and
    /// resolve the child component. This is the ONE place the component match +
    /// prop `name_span` containment + child resolution lives, so goto-definition
    /// (the props branch of [`Self::try_component_contract_definition`]) and
    /// cross-file rename ([`Self::classify_child_prop_rename`]) cannot drift on it.
    ///
    /// Returns [`ChildPropUsageClass::NotChildProp`] when the cursor is not on a
    /// child component's prop name, or the child is not a resolvable component.
    pub(super) fn resolve_child_prop_usage_at_cursor(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> ChildPropUsageClass {
        let Some(doc) = self.documents.get(uri) else {
            return ChildPropUsageClass::NotChildProp;
        };
        let Some(analysis) = self.documents.get_analysis(uri) else {
            return ChildPropUsageClass::NotChildProp;
        };
        let Some(template) = analysis.template.as_ref() else {
            return ChildPropUsageClass::NotChildProp;
        };
        let Some(offset) = doc.line_index.position_to_offset(position) else {
            return ChildPropUsageClass::NotChildProp;
        };

        for element in &template.elements {
            if !element.is_component {
                continue;
            }
            let Some(component) = template.components.iter().find(|c| {
                offset >= c.span.start
                    && offset < c.span.end
                    && (c.name == element.tag || c.name == to_pascal_case(&element.tag))
            }) else {
                continue;
            };

            // The cursor must be on a prop NAME (not a value / directive).
            let Some(prop) = component
                .props
                .iter()
                .find(|prop| offset >= prop.name_span.start && offset < prop.name_span.end)
            else {
                continue;
            };

            let Some(child) = self.resolve_component_document_for_usage(uri, &analysis, component)
            else {
                continue;
            };

            let child_canonical = uri_to_canonical_id(&child.uri);
            let child_carrier_api_path =
                verter_workspace::carrier_api_provider_path(&child_canonical);

            return ChildPropUsageClass::Resolved(Box::new(ResolvedChildPropUsage {
                usage: ChildPropUsage {
                    parent_uri: uri.clone(),
                    parent_prop_name: prop.name.clone(),
                    parent_prop_name_span: prop.name_span,
                    parent_is_shorthand: prop.is_shorthand,
                    child_carrier_api_path,
                },
                child,
            }));
        }

        ChildPropUsageClass::NotChildProp
    }

    /// Resolve a `<Child prop=…>` usage to the prop's child `.vue` DECLARATION span
    /// — the child's `defineProps` MACRO field span ONLY. This is the SOLE safe
    /// rename declaration: it is the typed identity the API generator keys its
    /// source-map token on, so the synthesized child-declaration leg maps back onto
    /// the child `.vue` exactly. Template-only prop definitions, the shorthand
    /// parent binding, and the navigate-to-child-file fallback (all conveniences
    /// goto-definition offers) are NOT safe rename declarations and are
    /// deliberately NOT consulted here.
    ///
    /// Returns `None` when the prop is not declared in the child's macros.
    pub(super) fn resolve_child_macro_prop_declaration(
        &self,
        resolved: &ResolvedChildPropUsage,
    ) -> Option<verter_span::Span> {
        let requested = resolved.usage.parent_prop_name.as_str();
        let mut best: Option<(u8, verter_span::Span)> = None;
        for mac in resolved.child.analysis.macros.iter() {
            for field in &mac.prop_fields {
                if let Some(rank) = attr_name_match_rank(requested, &field.name) {
                    let better = match best {
                        None => true,
                        Some((best_rank, best_span)) => {
                            rank < best_rank
                                || (rank == best_rank
                                    && (field.span.start, field.span.end)
                                        < (best_span.start, best_span.end))
                        }
                    };
                    if better {
                        best = Some((rank, field.span));
                    }
                }
            }
        }
        best.map(|(_, span)| span)
    }

    /// Classify a rename position with respect to the cross-file `<Child prop=…>`
    /// rename — a 2-state [`ChildPropRenameClass`], NOT a lossy `Option`, so the
    /// caller distinguishes "not a child prop" from "a CONFIRMED child prop" (the
    /// latter is gated on EVERY return path and fails closed when its declaration is
    /// unresolved or the merged edit is incomplete — never a usage-only partial).
    ///
    /// SYNCHRONOUS declaration resolution covers the INLINE case ONLY: the prop's
    /// declaration is the child's `defineProps` MACRO field span (the typed identity
    /// the API generator keys its source-map token on), so it resolves to
    /// [`ChildPropDeclarationProof::Known`] WITH an `inline_decl_span` Verter can
    /// SYNTHESIZE the child-`.vue` rename leg from (a provider's own
    /// `textDocument/rename` may not enumerate it across the synthesized API surface
    /// — tsgo does not). A `defineProps<ImportedType>()` surface (props are a bare
    /// imported type ref with no inline member span) resolves here to
    /// [`ChildPropDeclarationProof::Unknown`]; the async caller then UPGRADES it to
    /// `Known` via [`Self::upgrade_imported_child_prop_declaration`] (a provider
    /// `get_definition` hop) or fails closed. Built on the SHARED
    /// [`Self::resolve_child_prop_usage_at_cursor`] +
    /// [`Self::resolve_child_macro_prop_declaration`], so it cannot drift from the
    /// goto-definition props branch.
    pub(super) fn classify_child_prop_rename(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> ChildPropRenameClass {
        let resolved = match self.resolve_child_prop_usage_at_cursor(uri, position) {
            ChildPropUsageClass::Resolved(resolved) => resolved,
            ChildPropUsageClass::NotChildProp => return ChildPropRenameClass::NotChildProp,
        };

        // The parent usage NAME range in the negotiated encoding — the initiating
        // edit the gate also proves present (a declaration-only result is also
        // incomplete). Computed off the parent doc's negotiated-encoding line index.
        let expected_parent_usage_range = self
            .documents
            .get(uri)
            .and_then(|doc| {
                location_from_span(uri, &doc.line_index, resolved.usage.parent_prop_name_span)
            })
            .map(|loc| loc.range);

        // INLINE declaration: the child's `defineProps` macro field span — the typed
        // identity the API generator keys its source-map token on. Present ⇒ a
        // `Known` declaration WITH the inline synthesis seam. ABSENT (e.g.
        // `defineProps<ImportedType>()`) ⇒ `Unknown` for now; the async caller
        // upgrades it via a provider `get_definition` hop, or fails closed.
        let declaration = match self.resolve_child_macro_prop_declaration(&resolved) {
            Some(child_prop_decl_span) => {
                // The child `.vue` decl RANGE in the negotiated encoding — the exact
                // position the completeness gate proves the merged `WorkspaceEdit`
                // edits. Computed via the SAME `location_from_span` path the
                // goto-definition props branch uses, off the child doc's
                // negotiated-encoding line index, so the gate's expected range is
                // byte-identical to where the synthesized leg maps the edit.
                let range = location_from_span(
                    &resolved.child.uri,
                    &resolved.child.line_index,
                    child_prop_decl_span,
                )
                .map(|loc| loc.range);
                ChildPropDeclarationProof::Known {
                    uri: resolved.child.uri.clone(),
                    range,
                    inline_decl_span: Some(child_prop_decl_span),
                }
            }
            None => ChildPropDeclarationProof::Unknown,
        };

        ChildPropRenameClass::Confirmed(Box::new(ConfirmedChildPropRename {
            usage: resolved.usage,
            expected_parent_usage_range,
            declaration,
        }))
    }

    /// UPGRADE a CONFIRMED child-prop rename whose declaration is still
    /// [`ChildPropDeclarationProof::Unknown`] (the `defineProps<ImportedType>()`
    /// imported-type case, which has no inline macro-field span) to a `Known`
    /// declaration target by a single provider `get_definition` hop.
    ///
    /// The declaration target is resolved by querying the provider for the
    /// DEFINITION at the SAME validated parent-usage TSX offset the rename uses (not
    /// the child's `___VERTER___defineProps_Type<…>` type-ref surface, which only
    /// reaches the type declaration and would then need a separate member locator).
    /// The provider resolves the prop usage to its member declaration in ONE hop —
    /// uniformly for inline and imported props — landing on the member's REAL byte
    /// range in whatever file owns it (the imported type's `.ts`/`.d.ts`).
    ///
    /// The returned location is mapped to a source `{uri, range}` through the SAME
    /// definition-merge mapping go-to-definition uses (`resolve_external_target_range`
    /// for a real file, the carrier source maps for a carrier surface) — never a
    /// re-implemented resolver. It is accepted as `Known` ONLY when it maps cleanly
    /// AND the resolved source range spells EXACTLY the prop name (the correctness
    /// TRIPWIRE — a name-equality VALIDATION of a resolved range, NOT a name search
    /// driving the result). If the mapping or the tripwire fails, the declaration
    /// stays `Unknown` and the completeness gate fails closed (never a usage-only
    /// partial).
    ///
    /// NO-OP for any class other than a `Confirmed` rename whose declaration is
    /// `Unknown` (an inline `Known` declaration already has its child `.vue` macro
    /// span; `NotChildProp` is not gated).
    pub(super) async fn upgrade_imported_child_prop_declaration(
        &self,
        rename_class: &mut ChildPropRenameClass,
        type_provider: &dyn crate::type_provider::traits::TypeProvider,
        parent_tsx_path: &str,
        parent_tsx_offset: u32,
    ) {
        // Only a confirmed rename with an UNKNOWN declaration needs the hop.
        let ChildPropRenameClass::Confirmed(target) = rename_class else {
            return;
        };
        if !matches!(target.declaration, ChildPropDeclarationProof::Unknown) {
            return;
        }
        let prop_name = target.usage.parent_prop_name.clone();

        // Pin the FOREIGN carrier IDE surfaces BEFORE the definition hop, so a
        // returned foreign carrier location maps through the generation this
        // request began against.
        let foreign_ide_set = self.capture_foreign_carrier_ide_set();

        // Resolve the declaration target via the provider DEFINITION hop, mapped to
        // a source `{uri, range}` exactly as go-to-definition maps it.
        let Ok(type_defs) = type_provider
            .get_definition(parent_tsx_path, parent_tsx_offset)
            .await
        else {
            return;
        };
        if type_defs.is_empty() {
            return;
        }

        // The initiating parent usage location(s) a resolved candidate must be
        // DISTINCT from: a provider whose cross-file project membership cannot reach
        // the imported member (tsgo does not, for an imported-type prop) resolves
        // `get_definition` back to the prop's OWN usage occurrence in the parent
        // carrier — that is NOT a declaration. Accepting it would make the gate's
        // declaration-leg and parent-usage-leg checks pass on the SAME parent edit,
        // re-opening the usage-only-partial leak. The selector rejects any candidate
        // equal to one of these, so a usage-resolved target leaves the declaration
        // `Unknown` and the gate fails closed — the architecturally-honest outcome for
        // a provider that cannot resolve the cross-file member.
        let parent_usage_ranges: Vec<(Uri, Range)> = target
            .expected_parent_usage_range
            .map(|range| (target.usage.parent_uri.clone(), range))
            .into_iter()
            .collect();

        let Some((uri, range)) = self.map_definition_to_validated_decl(
            type_defs,
            parent_tsx_path,
            &prop_name,
            &parent_usage_ranges,
            &foreign_ide_set,
        ) else {
            return;
        };

        target.declaration = ChildPropDeclarationProof::Known {
            uri,
            range: Some(range),
            // Imported-type member: the provider's own native rename edits the third
            // file — there is NO inline child-`.vue` member to synthesize.
            inline_decl_span: None,
        };
    }

    /// Map provider DEFINITION locations (from the parent-usage hop) to the single
    /// validated declaration `{uri, range}` — the imported-type member declaration
    /// the completeness gate proves the provider's own rename edits.
    ///
    /// Routes the FULL location set through the SAME definition-merge mapping
    /// go-to-definition uses (carrier IDE/API surfaces map through their own source
    /// maps; every other target reads its own source and converts byte offsets) — no
    /// second resolver, ONE merge over the whole set so its cross-candidate dedup +
    /// non-carrier preference apply. Then [`select_validated_declaration`] picks the
    /// declaration: it considers ALL mapped candidates (never just the first) and
    /// accepts one only when it is DISTINCT from the initiating parent usage AND its
    /// resolved source range spells EXACTLY the prop name (the correctness TRIPWIRE: a
    /// name-equality VALIDATION of the resolved range). `None` when nothing maps
    /// cleanly or no mapped candidate is an accepted declaration (fail closed).
    fn map_definition_to_validated_decl(
        &self,
        type_defs: Vec<crate::type_provider::protocol::TypeLocation>,
        parent_tsx_path: &str,
        prop_name: &str,
        parent_usage_ranges: &[(Uri, Range)],
        foreign_ide_set: &crate::provider_surface_store::ProviderQuerySnapshot,
    ) -> Option<(Uri, Range)> {
        use crate::server::handler_guard::block_in_place_if_available;

        let encoding = self.position_encoding.read().clone();
        let host = &self.documents.host;
        let carrier_source_exists = |p: &str| host.get_source(p).is_some();

        // The current-request mapper context (the parent's IDE TSX). A foreign
        // carrier surface routes through its own context via the external resolver.
        let ctx = self.type_provider_context_for_path(parent_tsx_path)?;
        let source_reader = |p: &str| {
            block_in_place_if_available(|| self.documents.host().workspace_read().read_file(p))
        };

        // ONE merge over the WHOLE location set (not per-location): its cross-candidate
        // dedup + non-carrier preference apply, and the full mapped set is preserved so
        // a leading non-declaration candidate cannot discard a later real declaration.
        let mapped = merge::merge_definitions(
            None,
            type_defs,
            &ctx.tsx_path,
            &ctx.tsx_line_index,
            &ctx.mapper,
            &ctx.carrier_line_index,
            Some(&|ide_path: &str| self.foreign_ide_context(foreign_ide_set, ide_path)),
            // A sentinel document URI that no real target equals, so a same-file
            // short-circuit never fires (this is a definition merge over foreign
            // locations, never the queried file's own surface).
            &Self::definition_merge_sentinel_uri(),
            &carrier_source_exists,
            encoding.clone(),
            &source_reader,
        );

        // Consider ALL mapped candidates; accept one only when it is a DISTINCT
        // location from the parent usage AND its resolved range spells the prop name.
        select_validated_declaration(
            all_locations_of(mapped),
            parent_usage_ranges,
            prop_name,
            |uri, range, name| self.resolved_range_spells(uri, range, name),
        )
    }

    /// A sentinel document URI for the single-location definition merge in
    /// [`Self::map_definition_to_validated_decl`]: it must never equal a real target,
    /// so the merge's same-file short-circuit cannot fire.
    fn definition_merge_sentinel_uri() -> Uri {
        // A scheme no `file://` target can match.
        "verter-decl-merge-sentinel:///"
            .parse()
            .expect("static sentinel URI parses")
    }

    /// Build a [`TypeProviderContext`] for an explicit provider TSX path (the parent
    /// usage surface the rename queried), reusing the shared per-URI context builder.
    fn type_provider_context_for_path(&self, tsx_path: &str) -> Option<TypeProviderContext> {
        let uri = self.carrier_uri_from_ide_path(tsx_path)?;
        self.type_provider_context(&uri)
    }

    /// Whether the resolved source `range` in `uri` spells EXACTLY `expected_name`.
    /// Reads the target's own source through the host VFS (the LSP's single
    /// source-read authority) and slices the byte range the negotiated-encoding line
    /// index resolves. The correctness tripwire for the imported-type declaration
    /// hop — fail closed (`false`) on any read/convert/slice miss.
    pub(super) fn resolved_range_spells(
        &self,
        uri: &Uri,
        range: Range,
        expected_name: &str,
    ) -> bool {
        use crate::server::handler_guard::block_in_place_if_available;

        let canonical = uri_to_canonical_id(uri);
        let Some(source) = block_in_place_if_available(|| {
            self.documents.host().workspace_read().read_file(&canonical)
        }) else {
            return false;
        };
        let encoding = self.position_encoding.read().clone();
        let line_index = LineIndex::new(&source, encoding);
        let (Some(start), Some(end)) = (
            line_index.position_to_offset(&range.start),
            line_index.position_to_offset(&range.end),
        ) else {
            return false;
        };
        source
            .get(start as usize..end as usize)
            .is_some_and(|slice| slice == expected_name)
    }
}

#[cfg(test)]
mod select_validated_declaration_tests {
    use super::select_validated_declaration;
    use tower_lsp_server::ls_types::{Location, Position, Range, Uri};

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    fn rng(line: u32, start_ch: u32, end_ch: u32) -> Range {
        Range {
            start: Position {
                line,
                character: start_ch,
            },
            end: Position {
                line,
                character: end_ch,
            },
        }
    }

    fn loc(u: &str, range: Range) -> Location {
        Location { uri: uri(u), range }
    }

    /// A spelling check that accepts EVERY candidate (so the test isolates the
    /// candidate-iteration + usage-rejection behavior, not the name tripwire).
    fn always_spells(_uri: &Uri, _range: Range, _name: &str) -> bool {
        true
    }

    /// The initiating parent usage: `App.vue` 3:9..3:12 (the `foo` prop usage on
    /// `<Child foo=…>`). A provider that cannot reach the imported member can resolve
    /// `get_definition` back to THIS occurrence — it is NOT a declaration.
    fn parent_usage() -> (Uri, Range) {
        (uri("file:///src/App.vue"), rng(3, 9, 12))
    }

    /// The REAL imported-type member declaration in a THIRD file.
    fn third_file_member() -> Location {
        loc("file:///src/importedProps.ts", rng(5, 11, 14))
    }

    #[test]
    fn considers_all_candidates_accepts_later_real_declaration() {
        // The provider returned TWO mapped candidates for the usage hop: the FIRST is
        // the parent's own self-usage occurrence (NOT a declaration — must be
        // rejected), the SECOND is the real third-file member declaration (a distinct
        // location that spells the prop name — must be accepted).
        //
        // DISCRIMINATING: a first-candidate-only selector would inspect only the
        // parent self-usage, reject it, and return None — under-resolving a
        // genuinely-resolvable imported declaration. Considering ALL candidates
        // accepts the later real declaration.
        let parent = parent_usage();
        let mapped = vec![
            loc(parent.0.as_str(), parent.1), // parent self-usage — rejected
            third_file_member(),              // real declaration — accepted
        ];

        let selected = select_validated_declaration(
            mapped,
            std::slice::from_ref(&parent),
            "foo",
            always_spells,
        );

        let third = third_file_member();
        assert_eq!(
            selected,
            Some((third.uri, third.range)),
            "considering all candidates must accept the later real third-file member \
             declaration after rejecting the leading parent self-usage"
        );
    }

    #[test]
    fn all_candidates_are_parent_usage_locations_fails_closed_to_none() {
        // EVERY mapped candidate coincides with the initiating parent usage (the
        // provider could not reach the cross-file member and resolved only to the
        // usage occurrences). None is a declaration → the selector must return None so
        // the completeness gate fails closed (no usage-only partial).
        //
        // DISCRIMINATING: a selector that promoted a same-name usage location to a
        // declaration would return Some(parent-usage) here and re-open the
        // usage-only-partial leak.
        let parent = parent_usage();
        let mapped = vec![
            loc(parent.0.as_str(), parent.1),
            loc(parent.0.as_str(), parent.1),
        ];

        let selected = select_validated_declaration(
            mapped,
            std::slice::from_ref(&parent),
            "foo",
            always_spells,
        );

        assert!(
            selected.is_none(),
            "when every mapped candidate is the parent usage location the declaration \
             must stay unresolved (None) — fail closed"
        );
    }

    #[test]
    fn candidate_not_spelling_prop_name_is_rejected() {
        // A mapped candidate whose resolved range does NOT spell the prop name fails
        // the tripwire and must be skipped; the later candidate that DOES spell it is
        // accepted. Proves the name-equality VALIDATION still gates each candidate.
        let parent = parent_usage();
        // Spelling check: only the third-file member range spells `foo`.
        let third = third_file_member();
        let third_for_closure = third.clone();
        let spells = move |u: &Uri, range: Range, name: &str| {
            *u == third_for_closure.uri && range == third_for_closure.range && name == "foo"
        };
        let mapped = vec![
            loc("file:///src/other.ts", rng(1, 0, 3)), // a same-position non-match — fails tripwire
            third.clone(),                             // spells `foo` — accepted
        ];

        let selected =
            select_validated_declaration(mapped, std::slice::from_ref(&parent), "foo", spells);

        assert_eq!(
            selected,
            Some((third.uri, third.range)),
            "a candidate that does not spell the prop name is skipped; the one that does \
             is accepted"
        );
    }
}
