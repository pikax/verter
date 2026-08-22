//! Provider-sync state CRUD + context helpers.
//!
//! Inherent-impl extension methods on [`super::VerterLanguageServer`]
//! covering MRU bookkeeping, snapshot-pending queue, transition
//! preparation/commit, sync-state removal, type-provider context
//! materialisation, and virtual-file routing context.
//!
//! All methods were moved verbatim from `server.rs` (now `server/mod.rs`)
//! lines 2516-2857 + the trailing `virtual_file_context` helper. No
//! behaviour change. The sibling lives as a private child module under
//! `server/mod.rs` so it sees the parent's private struct fields without
//! visibility widening.

use std::collections::HashSet;

use tower_lsp_server::ls_types::Uri;

use crate::documents::line_index::LineIndex;
use crate::provider_sync::{
    commit_sync_transition, prepare_sync_transition, ProviderPathKind, ProviderSyncState,
};
use crate::type_provider::merge;

use super::server_utils::source_id_from_provider_carrier_path;
use super::{TypeProviderContext, VerterLanguageServer};

#[cfg(test)]
#[path = "workspace_symbol_frontier_tests.rs"]
mod workspace_symbol_frontier_tests;

impl VerterLanguageServer {
    /// Capture the immutable FOREIGN-carrier IDE surface set a provider-backed
    /// request pins BEFORE its provider query, so a returned foreign carrier
    /// location maps through the generation the request began against — never
    /// whatever surface is current at merge time.
    pub(super) fn capture_foreign_carrier_ide_set(
        &self,
    ) -> crate::provider_surface_store::ProviderQuerySnapshot {
        self.documents
            .provider_surfaces()
            .capture_current_carrier_ide_set()
    }

    /// Resolve the merge-time mapping context for a FOREIGN carrier IDE
    /// location from the pinned set `captured`
    /// ([`Self::capture_foreign_carrier_ide_set`]), fail-closed: an uncaptured
    /// path, a no-longer-honored surface, or a drifted foreign open document
    /// drops the location. Closed imported carriers use their captured
    /// source/map generation and remain navigable.
    pub(super) fn foreign_ide_context(
        &self,
        captured: &crate::provider_surface_store::ProviderQuerySnapshot,
        ide_path: &str,
    ) -> Option<merge::ExternalIdeContext> {
        let encoding = self.position_encoding.read().clone();
        crate::provider_surface_store::foreign_ide_context_from_captured(
            self.documents.provider_surfaces(),
            &self.documents,
            captured,
            ide_path,
            encoding,
        )
    }

    /// THE server-side record choke point for an API-surface sync.
    ///
    /// Records a fresh generation pinning the EXACT `api_code` synced under
    /// `dts_path`, together with the source map parsed from the SAME content.
    /// When the caller already holds the synced content's source map it passes it
    /// in `source_map_json`; otherwise (`None`) the live `get_public_api()` map is
    /// used ONLY when its code byte-matches `api_code`, so a snapshot never pairs
    /// the synced offsets with a source map produced against drifted content.
    pub(super) fn record_carrier_api_snapshot(
        &self,
        canonical_id: &str,
        dts_path: &str,
        api_code: &str,
        source_map_json: Option<&str>,
    ) {
        let store = self.documents.provider_surfaces();
        let host = self.documents.host();
        match source_map_json {
            Some(_) => crate::provider_surface_store::record_carrier_api_surface(
                store,
                Some(&self.documents),
                host,
                canonical_id,
                dts_path,
                api_code,
                source_map_json,
            ),
            // No map in scope → use the live map only if it still matches content.
            None => crate::provider_surface_store::record_carrier_api_surface_code_only(
                store,
                Some(&self.documents),
                host,
                canonical_id,
                dts_path,
                api_code,
            ),
        }
    }

    /// THE server-side record choke point for a DIRECT IDE-surface sync (the
    /// tsgo direct-open / bootstrap-unresolved paths; the tsserver publish path
    /// records through `record_and_version_carrier_companions` inside the
    /// carrier-sync gateway).
    ///
    /// Records a fresh generation pinning the EXACT provider bytes synced under
    /// `ide_path`, together with the source map produced for the canonical IDE
    /// artifact. Managed tsgo may replace the compiler-owned, wholly unmapped
    /// first prelude line with its owner-bound Svelte JSX adapter; that preserves
    /// every mapped coordinate while making the provider bytes intentionally
    /// differ from `ide_code` on that one line.
    /// When the caller already holds the synced content's source map it passes
    /// it in `source_map_json`; otherwise (`None`) the live IDE artifact's map
    /// is used ONLY when its code byte-matches `ide_code`, so a snapshot never
    /// pairs the synced offsets with a source map produced against drifted
    /// content. Called ONLY after a SUCCESSFUL provider sync (fail-closed:
    /// a failed sync records nothing).
    ///
    /// Returns whether the surface was actually recorded — the structural
    /// backstop in `record_carrier_ide_snapshot_inner` can refuse (`false`)
    /// even here, when the document turns out open despite no pin having
    /// been captured. Callers whose retry/requeue discipline depends on the
    /// record having actually happened must check this, not assume success.
    #[must_use]
    pub(super) fn record_carrier_ide_snapshot(
        &self,
        canonical_id: &str,
        ide_path: &str,
        ide_code: &str,
        source_map_json: Option<&str>,
    ) -> bool {
        self.record_carrier_ide_snapshot_inner(
            None,
            canonical_id,
            ide_path,
            ide_code,
            source_map_json,
        )
    }

    /// Record retained IDE output only if `revision` is still the live open
    /// document at the store-write linearization point.
    pub(super) fn record_carrier_ide_snapshot_if_current(
        &self,
        uri: &Uri,
        revision: &crate::documents::DocumentSnapshotIdentity,
        canonical_id: &str,
        ide_path: &str,
        ide_code: &str,
        source_map_json: Option<&str>,
    ) -> bool {
        self.record_carrier_ide_snapshot_inner(
            Some((uri, revision)),
            canonical_id,
            ide_path,
            ide_code,
            source_map_json,
        )
    }

    /// Dispatch to whichever of [`Self::record_carrier_ide_snapshot_if_current`]
    /// / [`Self::record_carrier_ide_snapshot`] applies, based on whether the
    /// caller captured a pin. THE single entry a call site with a caller-
    /// supplied `open_pin` (captured before ITS OWN compile — see
    /// `DocumentRegistry::open_compile_pin`) should use, instead of choosing
    /// manually between the two — a manual choice is exactly how a confirmed-
    /// open call site ended up passing `None` and falling through to the
    /// unfenced path. Returns whether the surface was actually recorded.
    pub(super) fn record_carrier_ide_snapshot_with_pin(
        &self,
        open_pin: Option<(&Uri, &crate::documents::DocumentSnapshotIdentity)>,
        canonical_id: &str,
        ide_path: &str,
        ide_code: &str,
        source_map_json: Option<&str>,
    ) -> bool {
        self.record_carrier_ide_snapshot_inner(
            open_pin,
            canonical_id,
            ide_path,
            ide_code,
            source_map_json,
        )
    }

    fn record_carrier_ide_snapshot_inner(
        &self,
        retained: Option<(&Uri, &crate::documents::DocumentSnapshotIdentity)>,
        canonical_id: &str,
        ide_path: &str,
        ide_code: &str,
        source_map_json: Option<&str>,
    ) -> bool {
        let store = self.documents.provider_surfaces();
        let host = self.documents.host();
        let owned_map: Option<std::sync::Arc<str>> = match source_map_json {
            Some(_) => None,
            // No map in scope → use the live IDE artifact's map only if its
            // code still byte-matches the content that was actually synced.
            None => {
                let profile = self.documents.tsx_profile.read().clone();
                host.get_ide(canonical_id, &profile)
                    .filter(|ide| &*ide.code == ide_code)
                    .and_then(|ide| ide.source_map.clone())
            }
        };
        let map_json = source_map_json.or(owned_map.as_deref());
        // ONE value: the bytes the provider holds and the mapper describing them.
        // Recording anything else — notably the raw compiler bytes, or a
        // projection narrower than the one this engine's publication applies —
        // makes the recorded content hash disagree with the receipt-stamped one,
        // and the committed-surface gate then refuses every capture for this
        // source. `ProjectSync` owns that answer for the active engine.
        let delivered = match self.project_sync.as_ref() {
            Some(sync) => sync.carrier_provider_surface(ide_path, ide_code),
            None => {
                // No provider topology is bound, so no engine holds this buffer.
                // The shared carrier-import projection is the whole answer.
                let workspace = self.vfs_workspace.read().clone();
                let encoding = self.position_encoding.read().clone();
                match crate::carrier_provider_projection::prepare_carrier_provider_imports(
                    workspace.as_deref(),
                    canonical_id,
                    ide_code,
                    encoding,
                ) {
                    Ok(prepared) => Some(prepared),
                    Err(_) => return false,
                }
            }
        };
        // Fail closed: a buffer whose provider bytes cannot be modelled is not
        // recorded, so no request maps offsets through content no engine has.
        let Some(delivered) = delivered else {
            tracing::debug!(
                "record_carrier_ide_snapshot: no modellable provider surface for {ide_path}"
            );
            return false;
        };
        // Route both the retained (`Some`) and unretained (`None`) arms through
        // the SAME fenced choke point every other carrier-sync site uses: a
        // `None` retained pin is refused when the carrier turns out to be open
        // anyway (a close→open mid-flight transition, or a caller — like a
        // background-file path reached with an unexpectedly-open document —
        // that never captured a pin), never falls through to an unguarded
        // record. See `record_carrier_ide_surface_fenced`'s doc comment for the
        // full pin-before-compile invariant.
        crate::provider_surface_store::record_carrier_ide_surface_fenced(
            store,
            Some(&self.documents),
            host,
            canonical_id,
            ide_path,
            &delivered,
            map_json,
            retained,
        )
    }

    /// The bytes of the carrier's CURRENT public-API projection — the identity
    /// an API companion delivery would carry right now.
    ///
    /// `None` when the carrier projects no public API or the projection fails:
    /// an unknown identity never matches a delivered one, so the companion reads
    /// as not-current and a completeness-requiring request fails closed until a
    /// publication delivers a known one.
    pub(super) fn current_public_api_identity(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<str>> {
        let projected = super::handler_guard::block_in_place_if_available(|| {
            self.documents.host().get_public_api(canonical_id)
        });
        match projected {
            // The identity of what a delivery would carry: deliveries publish
            // the TS-labeled rendering (the `.verter.ts` companion is
            // TypeScript-labeled whatever the SFC's dialect), so the identity
            // must be those same bytes or a widened JavaScript Options-API
            // companion would never read as current.
            Ok(api) => api.map(|api| std::sync::Arc::clone(api.ts_labeled_code())),
            Err(error) => {
                crate::report_public_api_projection_error(
                    "current_public_api_identity",
                    canonical_id,
                    &error,
                );
                None
            }
        }
    }

    /// Pre-extracted data for type provider calls.
    /// All DashMap guards are dropped before this is returned, so it is safe
    /// to hold this across `.await` points without risking deadlock.
    pub(super) fn type_provider_context(&self, uri: &Uri) -> Option<TypeProviderContext> {
        // Route through the generalized projection context (serves BOTH the
        // carrier-IDE and self-file rune-module projections). The feature layer
        // sees the same `tsx_*` field names regardless of projection. Every
        // field — path, content, mapper, both line indexes — comes from the ONE
        // captured immutable provider surface carried in `snapshot`.
        let ctx = self.provider_projection_context(uri)?;
        Some(TypeProviderContext {
            tsx_path: ctx.provider_path,
            tsx_content: ctx.provider_content,
            mapper: ctx.mapper,
            tsx_line_index: ctx.provider_line_index,
            carrier_line_index: ctx.source_line_index,
            snapshot: ctx.snapshot,
        })
    }

    /// Capture a current interactive provider surface, repairing the authored
    /// file first when `didChange` has advanced beyond the committed snapshot.
    /// Every provider-backed interactive feature uses this entry point so tsgo
    /// and tsserver have identical immediate-post-edit behavior.
    ///
    /// A PROJECTION-LESS carrier (failed open-time compile, or the startup
    /// race) is also repaired here — the full current-file repair
    /// (`ensure_current_file_synced`: recompile → carrier gateway → provider
    /// sync → surface record → commit), never a cache-only mapper install.
    /// The generation-local repair lane bounds a concurrent request storm to
    /// one compile. A genuinely later request retries unchanged bytes after an
    /// unavailable-input or host-transient failure, so scheduler cancellation
    /// cannot strand the carrier behind a durable failure memo. Only a
    /// deterministic source-byte verdict is retained across requests.
    ///
    /// FEATURE SET: hover, completion, definition, type-definition,
    /// references, prepare-rename/rename, signature help, and quickfix code
    /// actions repair through this entry point — every request-answering
    /// provider-backed feature. Passive decoration surfaces (document
    /// highlights, semantic tokens, inlay hints, code lens) deliberately do
    /// NOT: they fire on cursor-move/render cadence, so an inline repair
    /// there would put a compile on that cadence; they read the current
    /// surface (`type_provider_context`) and self-serve once a
    /// request-answering feature or the background coordinator installs it.
    pub(super) async fn repaired_type_provider_context(
        &self,
        uri: &Uri,
    ) -> Option<TypeProviderContext> {
        if self.type_provider.is_some() && self.current_file_needs_inline_type_provider_sync(uri) {
            self.ensure_current_file_synced(uri).await;
        }
        self.type_provider_context(uri)
    }

    /// Make a workspace-symbol query complete without paying the all-carrier
    /// cost during startup or ordinary hover/completion. TypeScript
    /// references/rename can only prove a project-wide answer when every
    /// framework source in the owning configured project is a Program root.
    /// The official framework-plugin model keeps that full set external; Verter
    /// admits the same set lazily on the first workspace-symbol request and the
    /// provider batches it behind one plugin refresh.
    ///
    /// Returns `false` instead of waiting for background compilation when the
    /// resolver snapshot or any carrier advertisement is not ready. Callers then
    /// fail closed (rename must never emit a partial edit; references must never
    /// claim a partial workspace result). Vue and Svelte share the descriptor-
    /// owned `path_is_carrier` classification.
    pub(super) async fn prepare_workspace_symbol_frontier(&self, uri: &Uri) -> bool {
        let Some(coordinator) = &self.carrier_publish_coordinator else {
            // Embedders/tests may inject a provider without the
            // managed store topology. There is no Verter-owned frontier to
            // prepare on that route; delegate completeness to that provider.
            return true;
        };
        let canonical = crate::documents::uri_to_canonical_id(uri);
        let (expected_sources, owner_key) = {
            // The server-side workspace is the publication authority used by
            // the scanner and membership reconciler. Test embedders may install
            // that handle without repointing the semantic host, while production
            // keeps both handles identical. Prefer it and retain the host only
            // as a compatibility fallback for embedders without a scanner VFS.
            let published = self
                .vfs_workspace
                .read()
                .as_ref()
                .and_then(|workspace| workspace.load_published())
                .or_else(|| self.documents.host().workspace_read().published_root());
            let Some(published) = published else {
                return false;
            };
            if !published.ownership_ready {
                return false;
            }
            let snapshot = &published.snapshot;
            let verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Unique(owner) =
                snapshot.configured_owner_resolution_for_file(&canonical)
            else {
                return false;
            };
            let Some(project) = snapshot.projects.get(owner.0 as usize) else {
                return false;
            };
            let verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                tsconfig_path,
                membership,
                ..
            } = &project.payload
            else {
                return false;
            };
            let mut sources: Vec<String> = membership
                .materialized_files
                .iter()
                .map(|path| path.as_str().to_string())
                .filter(|path| verter_workspace::resolver::path_is_carrier(path))
                .collect();
            if verter_workspace::resolver::path_is_carrier(&canonical) {
                sources.push(canonical.clone());
            }
            sources.sort_unstable();
            sources.dedup();
            (sources, tsconfig_path.as_str().to_string())
        };

        if expected_sources.is_empty() {
            return true;
        }
        let activation = if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
            // TSGO consumes explicit companion buffers. The editor-facing
            // tsserver store can be fully advertised while those local opens
            // are still parked, so only receipt-gated direct-open state proves
            // this provider's project graph is complete.
            //
            // The frontier's completeness unit is the carrier IDE companion —
            // the buffer holding the carrier's script and template symbols, and
            // therefore the one that must be a Program root before a
            // project-wide references/rename answer can be proven. That is
            // exactly what the tsserver arm below admits: `activate_published_sources`
            // promotes `SnapshotRole::CarrierIde` members only, and counts a
            // source as activated only when it has one. The tsgo arm mirrors
            // that unit. The API companion is the *import target* projection
            // consumed by files that import a carrier; the background API-sync
            // task opens it for imported carriers, and the interactive IDE-sync
            // path deliberately never opens it for the file under the cursor.
            // Requiring it here would gate the frontier on a companion neither
            // arm activates and the current file never gets.
            Ok(expected_sources
                .iter()
                .filter(|source| {
                    self.provider_sync_states
                        .get(source.as_str())
                        .is_some_and(|state| {
                            state.owner_binding.owner_key() == Some(owner_key.as_str())
                                && state.ide_background_loaded
                                && state.commit_stamp.is_some()
                        })
                })
                .count())
        } else {
            coordinator
                .activate_published_sources(&expected_sources)
                .await
                .map_err(|error| error.to_string())
        };

        match activation {
            Ok(activated) if activated == expected_sources.len() => {
                // Roots are live. On tsgo the graph must also RESOLVE: its
                // buffers are explicit opens, so a carrier-import edge whose
                // target companion is unopened leaves the IMPORTER's use of
                // that symbol unresolved while both IDE roots look complete.
                if !matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo)
                    || self.carrier_import_closure_is_live(&expected_sources, &owner_key)
                {
                    return true;
                }
                tracing::debug!(
                    "workspace-symbol frontier not ready: carrier-import closure incomplete"
                );
                self.signal_frontier_scanner_priority(expected_sources)
                    .await;
                false
            }
            Ok(activated) => {
                tracing::debug!(
                    "workspace-symbol frontier not ready: activated {activated}/{} carriers",
                    expected_sources.len()
                );
                self.signal_frontier_scanner_priority(expected_sources)
                    .await;
                false
            }
            Err(error) => {
                tracing::warn!("workspace-symbol frontier activation failed: {error}");
                false
            }
        }
    }

    /// Make the incomplete owning-project carriers the scanner's next work
    /// without making this interactive request join background compilation.
    async fn signal_frontier_scanner_priority(&self, expected_sources: Vec<String>) {
        if let Some(scanner) = self.workspace_scanner.lock().await.as_ref() {
            for source in expected_sources {
                scanner.signal_priority(source);
            }
        }
    }

    /// Whether every EFFECTIVE carrier-import edge reachable from the owning
    /// project's carriers already has its provider surface live for this owner.
    ///
    /// This is a SEPARATE demand from the IDE-root predicate above, and it gates
    /// a different failure: the roots being open proves each carrier's own
    /// symbols are in the Program, not that a cross-carrier reference RESOLVES.
    /// A parent's IDE buffer imports the rewritten `{child}.verter.ts` specifier,
    /// so while that API companion is unopened the parent's use of a child symbol
    /// is unresolved — and a project-wide references/rename answer comes back
    /// plausible but missing that file, which is worse than no answer.
    ///
    /// The demand is exactly what the background import-dependency publication
    /// delivers, so it can never gate on a surface nothing produces:
    /// - a carrier reached by a direct import (or a dynamic module reference)
    ///   needs its API companion open under a receipt-gated commit;
    /// - a barrel that RE-EXPORTS a carrier needs its rewritten shadow buffer
    ///   loaded — its `export … from './X.vue'` specifier is projected, so the
    ///   on-disk bytes do not resolve for the provider.
    ///
    /// A carrier that is NOT an import target is not gated (a standalone
    /// initiating carrier, and the file under the cursor, need only their IDE
    /// root); neither is any target outside this configured project, which the
    /// provider resolves from disk.
    fn carrier_import_closure_is_live(&self, expected_sources: &[String], owner_key: &str) -> bool {
        let published = self
            .vfs_workspace
            .read()
            .as_ref()
            .and_then(|workspace| workspace.load_published())
            .or_else(|| self.documents.host().workspace_read().published_root());
        let Some(published) = published else {
            return false;
        };
        let owned_by_frontier_project =
            |path: &str| configured_owner_key(&published.snapshot, path) == Some(owner_key);

        let host = self.documents.host();
        let resolver_snapshot = self.published_resolver();
        let reader = super::server_utils::LspProjectResolverReader::new(&self.documents);

        let mut carrier_targets: Vec<String> = Vec::new();
        let mut seen_carriers: HashSet<String> = HashSet::new();
        let mut barrel_frontier: Vec<String> = Vec::new();
        let mut seen_barrels: HashSet<String> = HashSet::new();
        let mut push_carrier = |target: String, carrier_targets: &mut Vec<String>| {
            if seen_carriers.insert(target.clone()) {
                carrier_targets.push(target);
            }
        };

        for source in expected_sources {
            let Some(ingress) = host.get_script_ingress(source) else {
                // The importer's edges cannot be enumerated from an unindexed
                // script: fail closed rather than assume it imports nothing.
                return false;
            };
            let direct_targets =
                match super::server_utils::collect_imported_carrier_priority_ids_from_imports_for_publication(
                    &ingress.imports,
                    Some(source.as_str()),
                    |parent, specifier| {
                        self.resolve_import_specifier_for_publication(parent, specifier)
                    },
                ) {
                    Ok(targets) => targets,
                    Err(_) => return false,
                };
            for target in direct_targets {
                push_carrier(target, &mut carrier_targets);
            }
            let Some(dynamic_targets) =
                super::server_utils::collect_priority_carrier_public_api_targets_from_module_references(
                resolver_snapshot.as_ref(),
                &reader,
                source,
                &ingress.module_references,
            )
            else {
                return false;
            };
            for target in dynamic_targets {
                push_carrier(target, &mut carrier_targets);
            }
            for import in ingress.imports.iter() {
                let resolved = match import.resolved_canonical_id.clone() {
                    Some(resolved) => Some(resolved),
                    None => match self
                        .resolve_import_specifier_for_publication(source, &import.source)
                    {
                        verter_workspace::ResolutionPublication::Admitted(admitted) => {
                            admitted.into_result()
                        }
                        verter_workspace::ResolutionPublication::Refused(_) => return false,
                    },
                };
                let Some(resolved) = resolved else {
                    continue;
                };
                if verter_workspace::resolver::path_is_carrier(&resolved) {
                    continue;
                }
                if seen_barrels.insert(resolved.clone()) {
                    barrel_frontier.push(resolved);
                }
            }
        }

        // Follow the `export … from` graph the publication's barrel leg follows,
        // classifying each hop by its RESOLVED target (never the specifier text),
        // so an aliased or multi-hop re-export reaches the terminal carrier.
        // `seen_barrels` terminates cycles.
        let mut rewritten_barrels: Vec<String> = Vec::new();
        while let Some(barrel) = barrel_frontier.pop() {
            if !owned_by_frontier_project(&barrel) {
                // A package-backed or foreign-project module: the provider
                // resolves it from disk and publication never rewrites it.
                continue;
            }
            let Some(ingress) = host.get_script_ingress(&barrel) else {
                return false;
            };
            let mut re_exports_carrier = false;
            for module_reference in ingress.module_references.iter() {
                if module_reference.syntax
                    != verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom
                {
                    continue;
                }
                let Some(specifier) = module_reference.literal_specifier.as_deref() else {
                    continue;
                };
                let target = match self.resolve_import_specifier_for_publication(&barrel, specifier)
                {
                    verter_workspace::ResolutionPublication::Admitted(admitted) => {
                        let Some(target) = admitted.into_result() else {
                            continue;
                        };
                        target
                    }
                    verter_workspace::ResolutionPublication::Refused(_) => return false,
                };
                if verter_workspace::resolver::path_is_carrier(&target) {
                    re_exports_carrier = true;
                    push_carrier(target, &mut carrier_targets);
                } else if seen_barrels.insert(target.clone()) {
                    barrel_frontier.push(target);
                }
            }
            if re_exports_carrier {
                rewritten_barrels.push(barrel);
            }
        }

        carrier_targets
            .iter()
            .filter(|target| owned_by_frontier_project(target))
            .all(|target| {
                self.provider_sync_states
                    .get(target.as_str())
                    .is_some_and(|state| {
                        state.owner_binding.owner_key() == Some(owner_key)
                            && state.commit_stamp.is_some()
                            // LIVENESS *and* CURRENCY. An open API buffer whose
                            // bytes predate the importer's edit resolves the
                            // importer's use of a renamed symbol to nothing, and
                            // the answer comes back missing that file — the same
                            // partial-but-plausible result an unopened companion
                            // produces, one stage later. The state-wide
                            // `commit_stamp` cannot stand in for this: an
                            // IDE-only receipt advances it while attesting no
                            // API bytes at all.
                            && state.api_companion_is_live_and_current()
                    })
            })
            && rewritten_barrels.iter().all(|barrel| {
                self.provider_sync_states
                    .get(barrel.as_str())
                    .is_some_and(|state| {
                        state.owner_binding.owner_key() == Some(owner_key)
                            // Same principle, cheaper key: the rewritten
                            // projection cannot be recomputed per request, so an
                            // edited barrel is not current until its buffer is
                            // re-delivered from these exact source bytes.
                            && host.get_source(barrel).is_some_and(|source| {
                                state.shadow_is_live_and_current(&source)
                            })
                    })
            })
    }

    /// Whether `uri` projects through a SELF-FILE rune-module own buffer.
    ///
    /// Features whose workspace-EDIT positions are not mapped through the
    /// self-file mapper (rename, code actions) are GATED OFF for a self-file
    /// projection — an unmapped edit would land off by the prelude offset (or
    /// inside the prelude) and corrupt the rune module. They stay DEFERRED for
    /// the self-file projection until their edit-mapping lands; the carrier
    /// projection is unaffected.
    pub(super) fn is_self_file_projection(&self, uri: &Uri) -> bool {
        self.documents
            .get_projection(uri)
            .is_some_and(|projection| projection.is_self_file())
    }

    /// Rename claimancy for `uri`, preserving the authority boundary between a
    /// published ownership graph and a transient bootstrap revision.
    ///
    /// A carrier is never classified from a missing/non-authoritative snapshot
    /// or while the root and provider generations disagree: an empty bootstrap
    /// graph and a rebuild transition both mean "not known coherently", not "no
    /// other claimant". Once ownership is coherent and authoritative, an
    /// overlap is `Ready`; a unique or unowned carrier is
    /// `NotMultiClaimant`. Non-carriers do not use this carrier-only gate.
    pub(super) fn carrier_multi_claimancy(&self, uri: &Uri) -> CarrierMultiClaimancy {
        let host = self.documents.host();
        let canonical = crate::documents::uri_to_canonical_id(uri);
        if !verter_workspace::resolver::path_is_carrier(&canonical) {
            return CarrierMultiClaimancy::NotMultiClaimant(None);
        }
        let Some(published) = host.workspace_read().published_root() else {
            return CarrierMultiClaimancy::NotReady;
        };
        let Some(witness) = self.ownership_generation_fence.capture(&published) else {
            return CarrierMultiClaimancy::NotReady;
        };
        if matches!(
            published
                .snapshot
                .configured_owner_resolution_for_file(&canonical),
            verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Ambiguous(_)
        ) {
            CarrierMultiClaimancy::Ready
        } else {
            CarrierMultiClaimancy::NotMultiClaimant(Some(witness))
        }
    }

    /// Revalidate a request's ownership witness against both live authorities.
    ///
    /// The root alone is insufficient during background rebuild: the provider
    /// authority moves first, while the previous ready root remains published.
    pub(super) fn ownership_generation_still_current(
        &self,
        witness: crate::configured_owner::OwnershipGenerationWitness,
    ) -> bool {
        let published = self.documents.host().workspace_read().published_root();
        self.ownership_generation_fence
            .validates(witness, published.as_deref())
    }

    /// Find the Vue URI corresponding to an IDE path.
    pub(super) fn carrier_uri_from_ide_path(&self, ide_path: &str) -> Option<Uri> {
        let snapshot = self.published_resolver()?;
        let canonical_id = source_id_from_provider_carrier_path(
            &snapshot.resolver,
            self.documents.host(),
            ide_path,
        )?;
        self.documents.canonical_id_to_uri(&canonical_id)
    }

    /// Touch a canonical ID in the MRU list (push to front, dedup).
    pub(super) fn touch_mru(&self, canonical_id: &str) {
        let mut mru = self.mru_canonical_ids.lock();
        mru.retain(|id| id != canonical_id);
        mru.insert(0, canonical_id.to_string());
        // Cap at a reasonable size
        mru.truncate(64);
    }

    pub(super) fn queue_snapshot_provider_sync(&self, canonical_id: impl Into<String>) {
        self.pending_snapshot_provider_sync
            .insert(canonical_id.into());
    }

    pub(super) fn provider_sync_state_for_source(
        &self,
        canonical_id: &str,
    ) -> Option<ProviderSyncState> {
        self.provider_sync_states
            .get(canonical_id)
            .map(|entry| entry.clone())
    }

    /// Route a carrier's sync through the SINGLE carrier-sync gateway: the membership
    /// decision (publish on owned / retract on owner-loss for tsserver) is FUSED with
    /// the provider-state transition + the sealed receipt that gates the commit. This
    /// is the server-side wrapper every interactive/background carrier-sync entry uses
    /// (it builds the engine membership context from `self`).
    ///
    /// `open_pin`, when the caller compiled `ide` against a currently-open
    /// document, MUST have been captured by the CALLER before that compile
    /// ran (never after — see `DocumentRegistry::open_compile_pin`). This
    /// function does NOT self-capture: the compile always happens in the
    /// caller, sometimes several statements or another async call before this
    /// is reached, so a pin captured HERE would reproduce the exact
    /// compile-to-identity gap the fenced record is meant to close (the pin
    /// would observe whatever the identity is at THIS call, not what produced
    /// `ide`). `None` when the caller had no live document to pin (a closed
    /// carrier, or a call site that genuinely never compiles against an open
    /// buffer).
    pub(super) async fn reconcile_carrier_via_gateway(
        &self,
        canonical_id: &str,
        is_jsx: bool,
        ide: Option<&verter_session::IdeResponse>,
        open_pin: Option<(&Uri, &crate::documents::DocumentSnapshotIdentity)>,
    ) -> crate::external_ts::CarrierSyncDecision {
        let Some(snapshot) = self.published_resolver() else {
            // No published snapshot yet (bootstrap): nothing to advertise/commit — a
            // settleable non-owned outcome the caller routes through the coordinator.
            return crate::external_ts::CarrierSyncDecision::NotOwned(
                crate::external_ts::CarrierNotOwned::pending(),
            );
        };
        // Clone the VFS handle out of the guard so no lock is held across the await.
        let vfs = self.vfs_workspace.read().clone();
        // tsserver: the carrier reaches the provider as a store-backed configured-
        // project member, so the gateway runs the membership reconcile. tsgo (no
        // coordinator) ⇒ `None` ⇒ the gateway returns a direct-open transition.
        let membership = self
            .carrier_publish_coordinator
            .as_ref()
            .map(|coordinator| crate::external_ts::CarrierMembershipCtx {
                coordinator,
                provider_delivery: if matches!(
                    self.type_provider_kind,
                    crate::TypeProviderKind::Tsgo
                ) {
                    crate::external_ts::CarrierProviderDelivery::DirectOpen
                } else {
                    crate::external_ts::CarrierProviderDelivery::StoreBacked
                },
                activate_provider_member: self
                    .documents
                    .canonical_id_to_uri(canonical_id)
                    .is_some(),
            });
        let publishes_editor_membership = membership.is_some();
        let decision =
            crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
                host: self.documents.host(),
                vfs: vfs.as_deref(),
                ownership_ready: snapshot.ownership_ready,
                resolver: &snapshot.resolver,
                provider_sync_states: &self.provider_sync_states,
                provider_surfaces: self.documents.provider_surfaces(),
                documents: Some(&self.documents),
                project_sync: self.project_sync.as_ref(),
                canonical_id,
                is_jsx,
                ide,
                open_pin,
                membership,
                admission: &self.carrier_transaction_coordinator,
                reason: crate::external_ts::ReconcileReason::SourceSynced,
            })
            .await;
        if publishes_editor_membership {
            self.notify_editor_carrier_store_changed().await;
        }
        decision
    }

    /// The carrier provider paths for `canonical_id` for the CLOSE-only path (delete /
    /// file-removed buffer cleanup). NOT a commit — needs no receipt.
    pub(super) fn carrier_close_state(
        &self,
        canonical_id: &str,
        is_jsx: bool,
    ) -> Option<ProviderSyncState> {
        let snapshot = self.published_resolver()?;
        let decl_path = self.documents.host().declaration_carrier_path(canonical_id);
        crate::external_ts::carrier_close_target(
            &snapshot.resolver,
            canonical_id,
            is_jsx,
            decl_path,
        )
    }

    pub(super) fn prepare_non_carrier_provider_sync_transition(
        &self,
        canonical_id: &str,
    ) -> Option<crate::provider_sync::ProviderSyncTransition> {
        let snapshot = self.published_resolver()?;
        let next_state = crate::provider_sync::non_carrier_sync_state_for_source(
            &snapshot.resolver,
            canonical_id,
        )?;
        Some(prepare_sync_transition(
            &self.provider_sync_states,
            canonical_id,
            next_state,
        ))
    }

    pub(super) fn commit_provider_sync_state(&self, canonical_id: &str, state: ProviderSyncState) {
        commit_sync_transition(&self.provider_sync_states, canonical_id, state);
    }

    /// Commit a CARRIER provider state through the coordinator's admission gate
    /// ([`crate::external_ts::CarrierTransactionCoordinator::admit_owned`]) — GATED on the
    /// sealed receipt minted by the carrier-sync gateway (so a carrier state can never be
    /// committed without the membership decision). On a `Superseded` refusal (stale /
    /// cross-owner / equal-key-different-artifact / owner-loss-since-capture) the source is
    /// requeued for a fresh transaction — the interactive callers' retry disposition, never
    /// silently dropped. Non-carrier (shadow) commits keep
    /// [`Self::commit_provider_sync_state`].
    pub(super) fn commit_carrier_provider_state(
        &self,
        canonical_id: &str,
        state: ProviderSyncState,
        receipt: &crate::external_ts::ProviderReadyReceipt,
    ) {
        if self.carrier_transaction_coordinator.admit_owned(
            self.documents.host(),
            &self.provider_sync_states,
            canonical_id,
            state,
            receipt,
        ) == crate::external_ts::AdmitOutcome::Superseded
        {
            self.queue_snapshot_provider_sync(canonical_id.to_string());
        }
    }

    pub(super) fn remove_provider_sync_state(
        &self,
        canonical_id: &str,
    ) -> Option<ProviderSyncState> {
        // Advance-before-mutate: removing a previously-committed carrier state is an
        // owner-loss for the admission barrier, so the coordinator advances the barrier
        // BEFORE it vacates the slot (a late owned token captured before the removal can
        // never resurrect the obsolete owner into the vacated slot — the barrier lives
        // outside the removed state). A non-carrier / uncommitted state removes without a
        // spurious advance.
        self.carrier_transaction_coordinator
            .advance_barrier_and_remove(&self.provider_sync_states, canonical_id)
    }

    pub(super) async fn clear_provider_sync_state(&self, canonical_id: &str) {
        if let Some(state) = self.remove_provider_sync_state(canonical_id) {
            self.close_provider_state(&state).await;
        }
    }

    /// Preserve (or create) an OPEN Vue document's unresolved provider state
    /// when no project owns it, keeping its IDE TSX live in the provider.
    ///
    /// Editor-liveness invariant: an open Vue document keeps a usable TSX in the
    /// provider even while its owning project is unresolved. Builds the commit
    /// state through the shared [`open_unresolved_carrier_state`] primitive (forces
    /// `Unresolved`, preserves the owner-independent live IDE path, drops the
    /// owner-derived API path), syncs the IDE TSX when fresh `ide_code` is
    /// available, and commits. It NEVER removes the state or closes the TSX.
    pub(super) async fn preserve_open_unresolved_carrier(
        &self,
        canonical_id: &str,
        is_jsx: bool,
        ide_code: Option<&str>,
        retained: Option<(&Uri, &crate::documents::DocumentSnapshotIdentity)>,
    ) {
        let previous = self.provider_sync_state_for_source(canonical_id);
        // Converting a previously-committed OWNED carrier (it carried a commit stamp) to
        // Unresolved is an owner-loss for the admission barrier: advance it so a late owned
        // token — captured before this conversion — can never resurrect the obsolete owner
        // into the now-unstamped slot (the vacant-resurrection fence).
        if previous
            .as_ref()
            .is_some_and(|state| state.commit_stamp.is_some())
        {
            self.carrier_transaction_coordinator
                .advance_barrier(canonical_id);
        }
        // The DESIRED Unresolved target: owner-independent desired-extension IDE
        // path + the open-vs-update syncability hint. Binding forced
        // `Unresolved`, owner-derived API dropped.
        let target = crate::provider_sync::open_unresolved_carrier_state(
            previous.as_ref(),
            canonical_id,
            is_jsx,
        );

        // Attempt the desired IDE sync when fresh code is available (update-in-
        // place when the desired path is already live, else first-open).
        let mut ide_synced = false;
        if let (Some(sync), Some(ide_code), Some(ide_path)) =
            (&self.project_sync, ide_code, target.ide_path.clone())
        {
            #[cfg(test)]
            self.maybe_pause_ide_sync_before_provider_write(canonical_id)
                .await;
            if let Some((uri, revision)) = retained {
                if !self.retained_ide_response_is_current(uri, Some(revision)) {
                    self.needs_ide_sync.insert(canonical_id.to_string());
                    return;
                }
            }
            let result = if target.ide_background_loaded {
                sync.sync_tsx(&ide_path, ide_code).await
            } else {
                sync.open_tsx(&ide_path, ide_code).await
            };
            match result {
                Ok(()) => {
                    ide_synced = true;
                    // Record a fresh generation pinning the EXACT IDE bytes just
                    // synced (interactive queries capture this surface). No source
                    // map in scope → the choke attaches the live IDE artifact's
                    // map only if it still byte-matches `ide_code`.
                    #[cfg(test)]
                    self.maybe_pause_ide_sync_before_surface_record(canonical_id)
                        .await;
                    let recorded = if let Some((uri, revision)) = retained {
                        self.record_carrier_ide_snapshot_if_current(
                            uri,
                            revision,
                            canonical_id,
                            &ide_path,
                            ide_code,
                            None,
                        )
                    } else {
                        self.record_carrier_ide_snapshot(canonical_id, &ide_path, ide_code, None)
                    };
                    if !recorded {
                        self.needs_ide_sync.insert(canonical_id.to_string());
                        return;
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        "preserve_open_unresolved_carrier: failed to sync open unresolved IDE path \
                         {ide_path}: {error}"
                    );
                }
            }
        }

        // Build the committed state + close targets through the SAME per-kind
        // discipline the owner-resolved path uses: a non-synced IDE kind RETAINS
        // the prior LIVE path (never dropped to a dead/None path while the prior
        // is still open in the provider — rows 7 & 9), the owner-derived API is
        // dropped+closed unconditionally, and the orphaned prior IDE path is
        // closed ONLY after a successful flip (close-after-success).
        // An UNRESOLVED (owner-less) open-document liveness state is membership-free
        // (no publish to forget), so it commits through the plain non-carrier path —
        // the receipt gates only OWNED-publish commits.
        let commit = crate::provider_sync::open_unresolved_carrier_commit(
            previous.as_ref(),
            target,
            ide_synced,
        );
        self.commit_provider_sync_state(canonical_id, commit.committed);
        if let Some(dropped) = commit.dropped_api {
            self.close_provider_paths(std::slice::from_ref(&dropped))
                .await;
        }
        if let Some(stale) = commit.stale_ide_after_success {
            self.close_provider_paths(std::slice::from_ref(&stale))
                .await;
        }
    }

    pub(super) fn is_background_loaded_for_source_kind(
        &self,
        canonical_id: &str,
        kind: ProviderPathKind,
    ) -> bool {
        self.provider_sync_state_for_source(canonical_id)
            .map(|state| state.background_loaded_for_kind(kind))
            .unwrap_or(false)
    }

    /// Commit a (possibly partial) Vue provider-sync result with the
    /// close-AFTER-successful-sync discipline, shared by every owner-resolved
    /// `.vue` foreground/background sync method.
    ///
    /// Per-kind partial-failure gated: a kind whose replacement did NOT sync
    /// reverts to its previous live path (so the committed state never
    /// advertises an unsynced path); then the new state is committed and ONLY
    /// the genuinely-stale paths are closed (kind synced AND not active). On a
    /// total failure (`synced_kinds` empty) nothing is committed or closed —
    /// the previous state + provider paths are retained intact.
    ///
    /// The tsgo receipt is minted from `pending` HERE — after the empty-`synced_kinds`
    /// early return — so it is minted only once at least one companion buffer opened;
    /// on total failure the pending drops unconfirmed and no receipt exists.
    pub(super) async fn commit_and_close_after_sync(
        &self,
        canonical_id: &str,
        previous_state: Option<&ProviderSyncState>,
        mut committed_state: ProviderSyncState,
        stale_paths: &[(ProviderPathKind, String)],
        synced_kinds: &[ProviderPathKind],
        pending: crate::external_ts::PendingProviderReady,
    ) {
        if synced_kinds.is_empty() {
            return;
        }
        crate::provider_sync::revert_unsynced_kinds(
            &mut committed_state,
            previous_state,
            synced_kinds,
        );
        let genuinely_stale = crate::provider_sync::genuinely_stale_after_sync(
            stale_paths,
            &committed_state,
            synced_kinds,
        );
        // At least one kind opened: NOW mint the receipt (post-open), attesting EXACTLY
        // the kinds that actually opened this pass, and commit through the admission gate.
        let ide_surface = committed_state.ide_path.as_deref().and_then(|path| {
            self.project_sync
                .as_ref()
                .and_then(|sync| sync.synced_tsx_surface(path))
        });
        let receipt = pending.confirm_opened_with_ide_surface(synced_kinds, ide_surface);
        // Gate the stale-path close on ADMISSION: a `Superseded` commit (a newer
        // transaction reclaimed the source, or an owner-loss advanced the barrier) requeues
        // and closes NOTHING — the computed stale paths may be the newer transaction's LIVE
        // buffers. Only an admitted commit closes them.
        if self.carrier_transaction_coordinator.admit_owned(
            self.documents.host(),
            &self.provider_sync_states,
            canonical_id,
            committed_state,
            &receipt,
        ) == crate::external_ts::AdmitOutcome::Superseded
        {
            self.queue_snapshot_provider_sync(canonical_id.to_string());
            return;
        }
        self.close_provider_paths(&genuinely_stale).await;
    }

    pub(super) async fn close_provider_paths(&self, paths: &[(ProviderPathKind, String)]) {
        let Some(sync) = &self.project_sync else {
            return;
        };
        for (kind, path) in paths {
            // A `Decl` close is ROUTED through THE declaration-overlay lifecycle
            // owner — the SOLE authority that issues a provider `close_dts` for a
            // declaration overlay — so there is no second, UNGUARDED Decl-close path.
            // The owner serializes the close behind the overlay's path lock and
            // re-checks the overlay's reachability + close generation before the
            // destructive close: a still-referenced overlay (or one whose generation
            // advanced via a racing open) is skipped (closing it would strand an open
            // root on TS2307); a `Decl` path that is NOT a proactive overlay (no slot,
            // generation 0) closes through the same path. The owner needs no resolver
            // snapshot here — its per-path serialization (not a compensate-after-close
            // re-open) is what keeps a concurrent open consistent.
            if *kind == ProviderPathKind::Decl {
                let target = self.decl_overlay_owner.close_target_for(path);
                self.decl_overlay_owner
                    .guarded_close(
                        sync,
                        &self.provider_sync_states,
                        std::slice::from_ref(&target),
                    )
                    .await;
                continue;
            }
            // EVERY closing store-backed surface (IDE / API / Shadow) is no longer
            // the active synced virtual surface — retire its active generation
            // under a fresh close EPOCH (historical snapshots stay valid for any
            // in-flight rename that already captured them; the `Closing` state
            // keeps the path failing closed until the provider close is
            // CONFIRMED). Retiring only the API role would leave a closed IDE /
            // Shadow surface `Current`: after a `did_close`, a reopen of the same
            // text (before a successful re-sync) could then capture the stale
            // snapshot and serve a query against a CLOSED provider buffer.
            // Capture the epoch-stamped close token so the finalize is scoped to
            // THIS close.
            let close_token = self.documents.provider_surfaces().forget(path);
            let result = match kind {
                ProviderPathKind::Ide => sync.close_tsx(path).await,
                ProviderPathKind::Api => sync.close_dts(path).await,
                ProviderPathKind::Shadow => sync.close_file(path).await,
                // Delegated above (the guarded close is the SOLE Decl-close path).
                ProviderPathKind::Decl => unreachable!("Decl is delegated to the guarded close"),
            };
            match result {
                // Only a CONFIRMED close finalizes, and only via THIS close's
                // token — if the path was reopened (or retired again by a newer
                // close) during the await, the epoch no longer matches and the
                // finalize is a no-op (the fresh snapshot is preserved). On an
                // error the token is dropped, so the `Closing` state persists
                // (fail closed).
                Ok(()) => {
                    self.documents
                        .provider_surfaces()
                        .finalize_close(close_token);
                }
                Err(error) => {
                    tracing::warn!("failed to close provider path {path}: {error}");
                }
            }
        }
    }

    pub(super) async fn close_provider_state(&self, state: &ProviderSyncState) {
        let paths = state.active_paths();
        self.close_provider_paths(&paths).await;
    }

    /// Release a now-closed carrier ROOT from the proactive declaration-overlay
    /// graph: drop it from every overlay's reachability set and CLOSE every
    /// `.d.<ext>.ts` overlay no longer reachable from any open root.
    ///
    /// An overlay still reached by a DIFFERENT open root is retained (closing it
    /// would strand that root's bare carrier imports on TS2307). The closed
    /// overlays are also stripped from their owner carrier's committed provider
    /// state so the Decl kind does not linger as a falsely-live path.
    pub(super) async fn release_declaration_overlays_for_closed_root(&self, root_canonical: &str) {
        let now_unreferenced = self.decl_overlay_owner.release_root(root_canonical);
        if now_unreferenced.is_empty() {
            return;
        }
        // Route the Decl close through THE declaration-overlay lifecycle owner — the
        // SOLE path that issues a provider `close_dts` for a declaration overlay (the
        // closure pass's reconcile uses the same owner). It serializes the close
        // behind the overlay's path lock and re-checks reachability + the close
        // generation before the destructive close, so this did_close-side close can
        // never clobber a concurrent reopen by another still-open root (TS2307
        // stranding). It also strips the `Decl` kind from each owner carrier's
        // committed state for the overlays it actually closes.
        let Some(sync) = &self.project_sync else {
            return;
        };
        self.decl_overlay_owner
            .guarded_close(sync, &self.provider_sync_states, &now_unreferenced)
            .await;
    }

    /// Check if a URI is a virtual file and return its TSGO routing context.
    ///
    /// For virtual files (verter-virtual://), the content IS the TSX already.
    /// The cursor position is in TSX coordinates, so we can query TSGO directly
    /// without position mapping.
    ///
    /// Fail-closed gates (any miss ⇒ `None`):
    /// - the SOURCE document has no capturable request surface
    ///   ([`Self::capture_provider_request_surface`]);
    /// - the VIRTUAL document's bytes do not match the captured surface's
    ///   provider content — a stale virtual tab holding generation N while the
    ///   provider serves generation N+1 would compute offsets against content
    ///   the provider no longer holds (torn, not merely stale).
    ///
    /// The returned context carries the captured snapshot for the post-await
    /// gate ([`Self::virtual_request_surface_still_valid`]) every virtual-file
    /// branch runs before mapping/returning provider output.
    pub(super) fn virtual_file_context(&self, uri: &Uri) -> Option<VirtualFileContext> {
        let source_uri_str = self.documents.get_virtual_source_uri(uri)?;
        let source_uri: Uri = source_uri_str.parse().ok()?;

        // Resolve the provider path through the captured request surface of the
        // source document (fail closed when no consistent surface exists) —
        // never an independent committed-path read.
        let snapshot = self.capture_provider_request_surface(&source_uri)?;
        let tsx_path = snapshot.stamp.provider_path.to_string();

        // Build LineIndex from the virtual file's content (for offset
        // conversion) and require those bytes to MATCH the captured surface —
        // the offsets computed from a drifted virtual buffer would index
        // content the provider does not hold.
        let doc = self.documents.get(uri)?;
        if *doc.source != *snapshot.provider_content {
            return None;
        }
        let line_index = doc.line_index.as_ref().clone();

        Some(VirtualFileContext {
            tsx_path,
            line_index,
            snapshot,
        })
    }

    /// Post-await validation for a virtual-file provider query: the captured
    /// surface is still honored AND the virtual document still byte-matches
    /// the captured provider content. `false` ⇒ the provider response was
    /// produced against a surface that no longer matches the virtual tab —
    /// the branch must DROP the provider contribution (fail closed).
    pub(super) fn virtual_request_surface_still_valid(
        &self,
        uri: &Uri,
        ctx: &VirtualFileContext,
    ) -> bool {
        self.documents
            .provider_surfaces()
            .captured_snapshot_still_honored(&ctx.snapshot)
            && self
                .documents
                .get(uri)
                .is_some_and(|doc| *doc.source == *ctx.snapshot.provider_content)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CarrierMultiClaimancy {
    NotMultiClaimant(Option<crate::configured_owner::OwnershipGenerationWitness>),
    NotReady,
    Ready,
}

/// The tsconfig key of the ONE configured project that owns `path`, or `None`
/// when no single configured project does (unowned, ambiguous, or fallback-only).
fn configured_owner_key<'a>(
    snapshot: &'a verter_workspace::WorkspaceSnapshot,
    path: &str,
) -> Option<&'a str> {
    let verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Unique(owner) =
        snapshot.configured_owner_resolution_for_file(path)
    else {
        return None;
    };
    match &snapshot.projects.get(owner.0 as usize)?.payload {
        verter_workspace::workspace_snapshot::ProjectPayload::Configured {
            tsconfig_path, ..
        } => Some(tsconfig_path.as_str()),
        _ => None,
    }
}

/// The routing context for a `verter-virtual://` document's provider query:
/// the provider path and line index the offsets are computed against, plus the
/// captured request surface the post-await gate revalidates.
pub(super) struct VirtualFileContext {
    /// The provider path the query routes to (the captured surface's path).
    pub(super) tsx_path: String,
    /// Line index over the VIRTUAL document's bytes (byte-matched to the
    /// captured surface at capture).
    pub(super) line_index: LineIndex,
    /// The captured request surface for post-await revalidation.
    pub(super) snapshot: std::sync::Arc<crate::provider_surface_store::ProviderSurfaceSnapshot>,
}
