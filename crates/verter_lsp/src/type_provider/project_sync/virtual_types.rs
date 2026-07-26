//! Provider projection and lifecycle for TSX carriers and their adjacent
//! virtual `@verter/types` fallback.

use super::*;

impl ProjectSync {
    /// Produce the exact carrier bytes owned by this provider topology.
    ///
    /// Managed/editor-owned tsgo cannot add compiler options to a configured
    /// project through `workspace/didChangeConfiguration`; native tsgo treats
    /// that payload as user preferences. Compiler-owned automatic JSX runtimes
    /// are therefore adapted to owner-bound classic JSX namespaces in the
    /// provider buffer. Callers that record a provider surface use this method
    /// first so the recorded bytes are the exact bytes delivered to the engine.
    pub(super) fn prepare_tsx_surface(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<PreparedTsxContent, TypeProviderError> {
        let specialized = if matches!(self.kind, TypeProviderKind::Tsgo) {
            if let Some(prepared) =
                crate::svelte_assets::prepare_managed_tsgo_svelte_carrier(tsx_path, tsx_content)
                    .map_err(|error| {
                        TypeProviderError::new(format!(
                            "failed to prepare Svelte JSX provider assets for {tsx_path}: {error}"
                        ))
                    })?
            {
                Cow::Owned(prepared.content)
            } else {
                crate::vue_assets::prepare_managed_tsgo_vue_carrier(tsx_path, tsx_content)
                    .map(|prepared| {
                        prepared.map_or(Cow::Borrowed(tsx_content), |prepared| {
                            Cow::Owned(prepared.content)
                        })
                    })
                    .map_err(|error| {
                        TypeProviderError::new(format!(
                            "failed to prepare Vue JSX provider assets for {tsx_path}: {error}"
                        ))
                    })?
            }
        } else {
            Cow::Borrowed(tsx_content)
        };

        let Some(companion) =
            verter_session::framework::descriptor::classify_carrier_companion(tsx_path)
        else {
            return Ok(PreparedTsxContent {
                prepared: PreparedCarrierProviderContent::unprojected(
                    Arc::from(specialized.as_ref()),
                    tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
                ),
                virtual_verter_types_path: None,
            });
        };
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.read().clone());
        // ONE preparation produces the delivered bytes AND the mapper describing
        // them, so the ledger below can hand both to a recorder as one value.
        let surface = crate::carrier_provider_projection::prepare_carrier_provider_surface(
            workspace.as_deref(),
            &companion.source,
            tsx_path,
            specialized.as_ref(),
            tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
            matches!(self.kind, TypeProviderKind::Tsgo),
        );
        Ok(PreparedTsxContent {
            virtual_verter_types_path: surface.virtual_verter_types_path().map(str::to_owned),
            prepared: surface.into_prepared(),
        })
    }

    fn virtual_verter_types_lock(&self, tsx_path: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.virtual_verter_types_locks
            .entry(tsx_path.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    async fn publish_provider_file(
        &self,
        path: &str,
        content: &str,
        lane: ProviderLane,
        verb: ProviderFileVerb,
    ) -> Result<(), TypeProviderError> {
        match (lane, verb) {
            (ProviderLane::Foreground, ProviderFileVerb::Load) => {
                self.provider.load_file(path, content).await
            }
            (ProviderLane::Foreground, ProviderFileVerb::Open) => {
                self.provider.open_file(path, content).await
            }
            (ProviderLane::Foreground, ProviderFileVerb::Update) => {
                self.provider.update_file(path, content).await
            }
            (ProviderLane::Background, ProviderFileVerb::Load) => {
                self.provider.load_file_background(path, content).await
            }
            (ProviderLane::Background, ProviderFileVerb::Open) => {
                self.provider.open_file_background(path, content).await
            }
            (ProviderLane::Background, ProviderFileVerb::Update) => {
                self.provider.update_file_background(path, content).await
            }
            (ProviderLane::Normal, ProviderFileVerb::Load) => {
                self.provider.load_file_normal(path, content).await
            }
            (ProviderLane::Normal, ProviderFileVerb::Open) => {
                self.provider.open_file_normal(path, content).await
            }
            (ProviderLane::Normal, ProviderFileVerb::Update) => {
                self.provider.update_file_normal(path, content).await
            }
        }
    }

    pub(super) async fn publish_tsx(
        &self,
        tsx_path: &str,
        tsx_content: &str,
        lane: ProviderLane,
        verb: ProviderFileVerb,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }

        let lock = self.virtual_verter_types_lock(tsx_path);
        let _guard = lock.lock().await;
        let prepared = self.prepare_tsx_surface(tsx_path, tsx_content)?;
        let virtual_path = prepared.virtual_verter_types_path.as_deref();
        let virtual_was_live =
            virtual_path.is_some_and(|path| self.virtual_verter_types_paths.contains(path));

        // A carrier rewritten to the overlay may only be published after its
        // dependency is available.
        if let Some(path) = virtual_path {
            self.publish_provider_file(path, VERTER_TYPES_VIRTUAL_DTS, lane, verb)
                .await?;
            self.virtual_verter_types_paths.insert(path.to_owned());
        }

        let result = self
            .publish_provider_file(tsx_path, prepared.prepared.content().as_ref(), lane, verb)
            .await;
        if result.is_err() {
            // A dependency created solely for a failed carrier publication has
            // no live consumer. Preserve an older overlay because the provider
            // may still serve the previous carrier that imports it.
            if virtual_path.is_some() && !virtual_was_live {
                let _ = self.close_virtual_verter_types(tsx_path, lane).await;
            }
            return result;
        }

        self.record_delivered_carrier_surface(tsx_path, tsx_content, prepared.prepared);

        // When an installed package becomes available, publish the unrewritten
        // carrier first. Closing its old overlay before that update would break
        // the still-live previous carrier if the update failed.
        if virtual_path.is_none() {
            self.close_virtual_verter_types(tsx_path, lane).await?;
        }
        Ok(())
    }

    pub(super) async fn close_tsx_in_lane(
        &self,
        tsx_path: &str,
        lane: ProviderLane,
    ) -> Result<(), TypeProviderError> {
        let lock = self.virtual_verter_types_lock(tsx_path);
        let _guard = lock.lock().await;
        let result = match lane {
            ProviderLane::Foreground => self.provider.close_file(tsx_path).await,
            ProviderLane::Background => self.provider.close_file_background(tsx_path).await,
            ProviderLane::Normal => self.provider.close_file_normal(tsx_path).await,
        };
        if result.is_ok() {
            self.retract_delivered_carrier_surface(tsx_path);
            self.close_virtual_verter_types(tsx_path, lane).await?;
        }
        result
    }

    async fn close_virtual_verter_types(
        &self,
        tsx_path: &str,
        lane: ProviderLane,
    ) -> Result<(), TypeProviderError> {
        let path = format!("{tsx_path}.__verter_types.d.ts");
        if self.virtual_verter_types_paths.remove(&path).is_none() {
            return Ok(());
        }
        let result = match lane {
            ProviderLane::Foreground => self.provider.close_file(&path).await,
            ProviderLane::Background => self.provider.close_file_background(&path).await,
            ProviderLane::Normal => self.provider.close_file_normal(&path).await,
        };
        if result.is_err() {
            self.virtual_verter_types_paths.insert(path);
        }
        result
    }

    /// Load a Vue file's TSX into the type provider for import resolution only.
    /// Unlike `open_tsx`, this does NOT trigger diagnostics in providers that support it.
    ///
    /// Suppressed (no-op `Ok`) for tsserver: the carrier IDE companion is served
    /// to tsserver from the publish store, never loaded as content here.
    pub async fn load_tsx(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Foreground,
            ProviderFileVerb::Load,
        )
        .await
    }

    /// Sync a Vue file's TSX representation to the type provider.
    ///
    /// Suppressed (no-op `Ok`) for tsserver: the carrier IDE companion's content
    /// flows to tsserver through the publish store + plugin membership, not a
    /// direct `provider.update_file`.
    pub async fn sync_tsx(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Foreground,
            ProviderFileVerb::Update,
        )
        .await
    }

    /// Open a new TSX file in the type provider.
    ///
    /// Suppressed (no-op `Ok`) for tsserver: the carrier IDE companion becomes a
    /// configured-project member via the plugin's store-backed `getExternalFiles`,
    /// so the LSP must NOT open the synthetic companion as a second content
    /// authority.
    pub async fn open_tsx(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Foreground,
            ProviderFileVerb::Open,
        )
        .await
    }

    /// Close a TSX file in the type provider. Active for every engine — a close
    /// is provider state cleanup, never a carrier-content authority.
    pub async fn close_tsx(&self, tsx_path: &str) -> Result<(), TypeProviderError> {
        self.close_tsx_in_lane(tsx_path, ProviderLane::Foreground)
            .await
    }

    /// Register a published carrier companion with the provider so its queries
    /// route to the OWNING configured project (`projectFileName`) and convert
    /// positions against the carrier content — WITHOUT opening it as an editor
    /// buffer (the plugin's `getScriptSnapshot` stays the sole engine-side content
    /// authority; the `content` here is the provider's LOCAL position-conversion
    /// copy only, never forwarded to the engine). This is the carrier-membership
    /// query-routing signal for the tsserver engine — NOT a carrier-content open —
    /// so it is NOT suppressed. A no-op on engines that need neither (the trait
    /// default).
    pub async fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider
            .register_carrier_member(source_path, companion_path, content, project_file_name)
            .await
    }
}
