//! `TypeProvider` surface of the hub: state mutations go through the actor,
//! queries run against the serving incarnation under crash quarantine and
//! epoch settlement.

use super::quarantine::hash_extra;
use super::*;
use crate::protocol::*;
use crate::traits::ProviderFuture;

impl<P> TypeProvider for ProviderHub<P>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    fn provider_id(&self) -> &'static str {
        // The engine identity is stable across establishments (the establisher
        // always produces the same provider type), so read it from the serving
        // incarnation when present, else from the establisher's user label.
        self.state
            .shared
            .serving()
            .map(|serving| serving.provider.provider_id())
            .unwrap_or_else(|| self.state.establisher.user_label())
    }

    fn supports_completion_resolve(&self) -> bool {
        self.state
            .shared
            .serving()
            .map(|serving| serving.provider.supports_completion_resolve())
            .unwrap_or_else(|| self.state.establisher.supports_completion_resolve())
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Open { path, content }, Lane::Foreground)
                .await
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Load { path, content }, Lane::Foreground)
                .await
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Update { path, content }, Lane::Foreground)
                .await
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Close { path }, Lane::Foreground)
                .await
        })
    }

    fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
        // Pure engine-cache eviction signal; it changes no desired state, so it
        // bypasses the actor. An engine that is not serving holds no cache to
        // evict: the one that replaces it reads the carrier as it is.
        let path_owned = companion_path.to_string();
        Box::pin(async move {
            match self.state.shared.serving() {
                Some(serving) => serving.provider.notify_carrier_changed(&path_owned).await,
                None => Ok(()),
            }
        })
    }

    fn notify_carriers_changed<'a>(
        &'a self,
        companion_paths: &'a [String],
    ) -> ProviderFuture<'a, ()> {
        let paths_owned = companion_paths.to_vec();
        Box::pin(async move {
            match self.state.shared.serving() {
                Some(serving) => serving.provider.notify_carriers_changed(&paths_owned).await,
                None => Ok(()),
            }
        })
    }

    fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        let source_path = source_path.to_string();
        let companion_path = companion_path.to_string();
        let content = content.to_string();
        let project_file_name = project_file_name.to_string();
        Box::pin(async move {
            self.mutate(
                DesiredMutation::RegisterCarrier {
                    source_path,
                    companion_path,
                    content,
                    project_file_name,
                },
                Lane::Foreground,
            )
            .await
        })
    }

    fn register_carrier_metadata(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        let source_path = source_path.to_string();
        let companion_path = companion_path.to_string();
        let content = content.to_string();
        let project_file_name = project_file_name.to_string();
        Box::pin(async move {
            self.mutate(
                DesiredMutation::RegisterCarrierMetadata {
                    source_path,
                    companion_path,
                    content,
                    project_file_name,
                },
                Lane::Background,
            )
            .await
        })
    }

    fn activate_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        project_file_name: &str,
        script_kind: crate::traits::CarrierScriptKind,
    ) -> ProviderFuture<'_, ()> {
        let source_path = source_path.to_string();
        let companion_path = companion_path.to_string();
        let project_file_name = project_file_name.to_string();
        Box::pin(async move {
            self.mutate(
                DesiredMutation::ActivateCarrier {
                    source_path,
                    companion_path,
                    project_file_name,
                    script_kind,
                },
                Lane::Foreground,
            )
            .await
        })
    }

    fn activate_carrier_members<'a>(
        &'a self,
        members: &'a [crate::traits::CarrierActivation],
    ) -> ProviderFuture<'a, ()> {
        let members = members.to_vec();
        Box::pin(async move {
            self.mutate(
                DesiredMutation::ActivateCarriers { members },
                Lane::Foreground,
            )
            .await
        })
    }

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let path_owned = path.to_string();
        let trigger_owned = trigger_character.map(|s| s.to_string());
        let fp = QueryFingerprint::new(
            "completions",
            path,
            u64::from(offset),
            hash_extra(trigger_character.unwrap_or_default()),
        );
        Box::pin(async move {
            self.run_guarded(
                fp,
                || CompletionResult {
                    items: Vec::new(),
                    is_incomplete: false,
                },
                move |provider| async move {
                    provider
                        .get_completions(&path_owned, offset, trigger_owned.as_deref())
                        .await
                },
            )
            .await
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        let path_owned = path.to_string();
        let items = items.to_vec();
        // The fail-closed answer for a quarantined detail request is the
        // un-enriched list itself (mirrors the engine-less shape).
        let passthrough = items.clone();
        let fp = QueryFingerprint::new(
            "completion_details",
            path,
            u64::from(offset),
            hash_extra(
                &items
                    .iter()
                    .map(|item| item.label.as_str())
                    .collect::<Vec<_>>()
                    .join("\u{0}"),
            ),
        );
        Box::pin(async move {
            self.run_guarded(
                fp,
                move || passthrough,
                move |provider| async move {
                    provider
                        .get_completion_details(&path_owned, offset, &items)
                        .await
                },
            )
            .await
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("hover", path, u64::from(offset), 0);
        Box::pin(async move {
            self.run_guarded(
                fp,
                || None,
                move |provider| async move { provider.get_hover(&path_owned, offset).await },
            )
            .await
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("diagnostics", path, 0, 0);
        Box::pin(async move {
            self.run_guarded_with_fallback(
                fp,
                || {
                    Err(TypeProviderError::new(
                        "diagnostics quarantined after repeated provider crashes",
                    ))
                },
                move |provider| async move { provider.get_diagnostics(&path_owned).await },
            )
            .await
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("definition", path, u64::from(offset), 0);
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider.get_definition(&path_owned, offset).await
            })
            .await
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("type_definition", path, u64::from(offset), 0);
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider.get_type_definition(&path_owned, offset).await
            })
            .await
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("references", path, u64::from(offset), 0);
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider.get_references(&path_owned, offset).await
            })
            .await
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("rename_locations", path, u64::from(offset), 0);
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider.get_rename_locations(&path_owned, offset).await
            })
            .await
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("signature_help", path, u64::from(offset), 0);
        Box::pin(async move {
            self.run_guarded(fp, || None, move |provider| async move {
                provider.get_signature_help(&path_owned, offset).await
            })
            .await
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let path_owned = path.to_string();
        let diagnostics = diagnostics.to_vec();
        let fp = QueryFingerprint::new(
            "code_actions",
            path,
            u64::from(start_offset) | (u64::from(end_offset) << 32),
            0,
        );
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider
                    .get_code_actions(&path_owned, start_offset, end_offset, &diagnostics)
                    .await
            })
            .await
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("semantic_tokens", path, 0, 0);
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider.get_semantic_tokens(&path_owned).await
            })
            .await
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("document_highlights", path, u64::from(offset), 0);
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider.get_document_highlights(&path_owned, offset).await
            })
            .await
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new(
            "inlay_hints",
            path,
            u64::from(start_offset) | (u64::from(end_offset) << 32),
            0,
        );
        Box::pin(async move {
            self.run_guarded(fp, Vec::new, move |provider| async move {
                provider
                    .get_inlay_hints(&path_owned, start_offset, end_offset)
                    .await
            })
            .await
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new(
            "resolve_completion",
            path,
            0,
            hash_extra(&format!("{data:?}")),
        );
        Box::pin(async move {
            self.run_guarded(
                fp,
                || None,
                move |provider| async move { provider.resolve_completion(&path_owned, data).await },
            )
            .await
        })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async {
            self.shutdown_hub().await;
            Ok(())
        })
    }

    fn notify_watched_files_changed<'a>(
        &'a self,
        changes: &'a [crate::WatchedFileChange],
    ) -> ProviderFuture<'a, ()> {
        // Not desired state: an engine that is down has nothing to be told, and
        // the one that replaces it reads the disk as it is.
        Box::pin(async move {
            match self.state.shared.serving() {
                Some(serving) => serving.provider.notify_watched_files_changed(changes).await,
                None => Ok(()),
            }
        })
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        // Resync does not change the desired-state set, so it bypasses the actor
        // and runs directly against the serving engine (a fresh engine was just
        // replayed and has nothing to resync). Concurrency with an in-flight
        // `update_file` is made stale-safe at the provider (ipc) layer by its
        // per-file content generation gate.
        Box::pin(async move {
            match self.state.shared.serving() {
                Some(serving) => serving.provider.resync_open_files().await,
                None => Ok(()),
            }
        })
    }

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        let base_url = base_url.to_string();
        Box::pin(async move {
            self.mutate(
                DesiredMutation::ConfigurePaths { base_url, paths },
                Lane::Foreground,
            )
            .await
        })
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            self.mutate(
                DesiredMutation::UpdateWorkspaceFolders { added, removed },
                Lane::Foreground,
            )
            .await
        })
    }

    fn child_pid(&self) -> Option<u32> {
        self.state
            .shared
            .serving()
            .and_then(|serving| serving.provider.child_pid())
    }

    // ── Background-priority forwarding ──────────────────────────────

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Open { path, content }, Lane::Background)
                .await
        })
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Load { path, content }, Lane::Background)
                .await
        })
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Update { path, content }, Lane::Background)
                .await
        })
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Close { path }, Lane::Background)
                .await
        })
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path_owned = path.to_string();
        let fp = QueryFingerprint::new("diagnostics", path, 0, 0);
        Box::pin(async move {
            self.run_guarded_with_fallback(fp, || Err(TypeProviderError::new("diagnostics quarantined after repeated provider crashes")), move |provider| async move {
                provider.get_diagnostics_background(&path_owned).await
            })
            .await
        })
    }

    fn get_diagnostics_in_project<'a>(
        &'a self,
        path: &'a str,
        configured_project: &'a str,
    ) -> ProviderFuture<'a, Option<Vec<TypeDiagnostic>>> {
        let path_owned = path.to_string();
        let configured_project = configured_project.to_string();
        let fp = QueryFingerprint::new("diagnostics-in-project", path, 0, 0)
            .in_scope(&configured_project);
        Box::pin(async move {
            self.run_guarded_with_fallback(
                fp,
                || {
                    Err(TypeProviderError::new(
                        "diagnostics quarantined after repeated provider crashes",
                    ))
                },
                move |provider| async move {
                    provider
                        .get_diagnostics_in_project(&path_owned, &configured_project)
                        .await
                },
            )
            .await
        })
    }

    fn configure_paths_background(
        &self,
        base_url: &str,
        paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        let base_url = base_url.to_string();
        Box::pin(async move {
            self.mutate(
                DesiredMutation::ConfigurePaths { base_url, paths },
                Lane::Background,
            )
            .await
        })
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            self.mutate(
                DesiredMutation::UpdateWorkspaceFolders { added, removed },
                Lane::Background,
            )
            .await
        })
    }

    // ── Normal-priority forwarding ──────────────────────────────────

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Open { path, content }, Lane::Normal)
                .await
        })
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Load { path, content }, Lane::Normal)
                .await
        })
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Update { path, content }, Lane::Normal)
                .await
        })
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.mutate(DesiredMutation::Close { path }, Lane::Normal)
                .await
        })
    }
}
