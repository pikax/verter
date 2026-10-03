//! Provider projection and lifecycle for TSX carriers and their adjacent
//! virtual `@verter/types` fallback.

use super::*;

impl ProjectSync {
    /// API I/O owns only the path lock. The document transaction releases its
    /// lane before calling this and validates again after reacquiring it. The
    /// write keeps the foreground priority of the direct carrier-API open/update
    /// verbs; releasing the lane, not a lower hub priority, is what keeps an
    /// interactive repair from waiting on this round trip.
    pub(crate) async fn deliver_api_fenced(
        &self,
        path: &str,
        content: &str,
        update: bool,
        fence: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<SyncedApiSurface>, TypeProviderError> {
        let lock = self.virtual_verter_types_lock(path);
        let _guard = lock.lock().await;
        if !fence() || self.carrier_companion_open_suppressed() {
            return Ok(None);
        }
        let disposition = self
            .publish_provider_file(
                path,
                content,
                ProviderLane::Foreground,
                if update {
                    ProviderFileVerb::Update
                } else {
                    ProviderFileVerb::Open
                },
            )
            .await?;
        if disposition != verter_type_runtime::traits::FileLoadDisposition::Forwarded
            || !self.companion_applied_verbatim(path, content)
        {
            return Ok(None);
        }
        Ok(Some(SyncedApiSurface {
            path: Arc::from(path),
            content: Arc::from(content),
        }))
    }

    /// Drive held lazy demand from the background drain, then certify its exact
    /// delivery without republishing an older snapshot over a concurrent edit.
    pub(crate) async fn synchronize_pending_tsx(
        &self,
        path: &str,
        content: &str,
    ) -> Result<CarrierDelivery, TypeProviderError> {
        self.provider.synchronize_pending_file(path).await?;
        let lock = self.virtual_verter_types_lock(path);
        let _guard = lock.lock().await;
        let prepared = self.prepare_tsx_surface(path, content)?;
        let Some(delivered) = self.certified_delivery(path, prepared.prepared) else {
            return Ok(CarrierDelivery::Refused);
        };
        let receipt = SyncedTsxSurface::from_delivered(path, delivered.clone());
        self.record_delivered_carrier_surface(path, content, delivered);
        Ok(CarrierDelivery::Delivered(Some(receipt)))
    }

    /// Produce the exact carrier bytes owned by this provider topology.
    ///
    /// Managed/editor-owned tsgo cannot add compiler options to a configured
    /// project through `workspace/didChangeConfiguration`; native tsgo treats
    /// that payload as user preferences. Compiler-owned automatic JSX runtimes
    /// are therefore adapted to owner-bound classic JSX namespaces in the
    /// provider buffer. Callers that record a provider surface use this method
    /// first so the recorded bytes are the exact bytes delivered to the engine.
    ///
    /// This is also the ONE funnel every carrier preparation passes through, so
    /// a failure is recorded exactly once wherever it originates — the
    /// publication path (which propagates the error) and the read path (which
    /// fails closed to `None`) alike. The recorded reason is what
    /// [`ProjectSync::carrier_preparation_failure`] hands the diagnostic pass;
    /// without it a read-side failure would be observable only in a log.
    pub(super) fn prepare_tsx_surface(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<PreparedTsxContent, TypeProviderError> {
        let result = self.prepare_tsx_surface_inner(tsx_path, tsx_content);
        let key = super::carrier_failure_key(tsx_path);
        match &result {
            Ok(_) => {
                self.carrier_preparation_failures.remove(&key);
            }
            Err(error) => {
                tracing::error!(
                    "project_sync: carrier provider surface unavailable for {tsx_path}: {error}"
                );
                self.carrier_preparation_failures
                    .insert(key, Arc::from(error.message.as_str()));
            }
        }
        result
    }

    fn prepare_tsx_surface_inner(
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
        let surface = match crate::carrier_provider_projection::prepare_carrier_provider_surface(
            workspace.as_deref(),
            &companion.source,
            tsx_path,
            specialized.as_ref(),
            tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
            matches!(self.kind, TypeProviderKind::Tsgo),
        ) {
            Ok(surface) => surface,
            Err(refusal) => {
                return Err(TypeProviderError::new(format!(
                    "carrier provider projection was not admitted for {tsx_path}: {:?}",
                    refusal.reason()
                )));
            }
        };
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
    ) -> Result<verter_type_runtime::traits::FileLoadDisposition, TypeProviderError> {
        let priority = match lane {
            ProviderLane::Foreground => {
                verter_type_runtime::provider_hub::OverlayPriority::Foreground
            }
            ProviderLane::Normal => verter_type_runtime::provider_hub::OverlayPriority::Normal,
            ProviderLane::Background => {
                verter_type_runtime::provider_hub::OverlayPriority::Background
            }
        };
        match verb {
            ProviderFileVerb::Load => {
                self.provider
                    .load_file_with_disposition(path, content, priority)
                    .await
            }
            ProviderFileVerb::Open => {
                self.provider
                    .open_file_with_disposition(path, content, priority)
                    .await
            }
            ProviderFileVerb::Update => {
                self.provider
                    .update_file_with_disposition(path, content, priority)
                    .await
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
        self.publish_tsx_fenced(tsx_path, tsx_content, lane, verb, None)
            .await
            .map(|_| ())
    }

    /// [`Self::publish_tsx`] with a delivery fence: `fence` is evaluated UNDER
    /// the per-path delivery lock, immediately before the provider write. A
    /// `false` answer delivers nothing and answers
    /// [`CarrierDelivery::Refused`].
    ///
    /// The lock serializes every writer of one provider path (the interactive
    /// repair and the debounced coordinator both deliver here), so a writer
    /// that compiled an older document revision and then waited on the lock
    /// while a newer revision was delivered is refused at the only point that
    /// matters — the moment its bytes would overwrite the newer ones. A fence
    /// checked before taking the lock cannot see that: the coordinator's
    /// `hover_secondary_files_tsgo` flake was exactly a pre-edit compile
    /// written to tsgo after the foreground repair had delivered the edit, so
    /// the next hover mapped fresh offsets onto a stale buffer and fell back
    /// to the Verter-only answer.
    ///
    /// A success hands back the receipt for the content THIS call delivered, so
    /// the commit seals its own bytes instead of re-reading the path's ledger.
    pub(crate) async fn publish_tsx_fenced(
        &self,
        tsx_path: &str,
        tsx_content: &str,
        lane: ProviderLane,
        verb: ProviderFileVerb,
        fence: Option<&(dyn Fn() -> bool + Sync)>,
    ) -> Result<CarrierDelivery, TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            if fence.is_some_and(|fence| !fence()) {
                return Ok(CarrierDelivery::Refused);
            }
            let prepared = self.prepare_tsx_surface(tsx_path, tsx_content)?;
            return Ok(CarrierDelivery::Published(prepared.prepared));
        }

        let lock = self.virtual_verter_types_lock(tsx_path);
        let _guard = lock.lock().await;
        if let Some(fence) = fence {
            if !fence() {
                tracing::debug!(
                    "project_sync: not delivering {tsx_path} — its document revision moved \
                     before the provider write"
                );
                return Ok(CarrierDelivery::Refused);
            }
        }
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
        if matches!(
            &result,
            Ok(verter_type_runtime::traits::FileLoadDisposition::Shadowed
                | verter_type_runtime::traits::FileLoadDisposition::Held)
        ) {
            if virtual_path.is_some() && !virtual_was_live {
                self.close_virtual_verter_types(tsx_path, lane).await?;
            }
            return Ok(CarrierDelivery::Refused);
        }
        if let Err(error) = result {
            // A dependency created solely for a failed carrier publication has
            // no live consumer. Preserve an older overlay because the provider
            // may still serve the previous carrier that imports it.
            if virtual_path.is_some() && !virtual_was_live {
                let _ = self.close_virtual_verter_types(tsx_path, lane).await;
            }
            return Err(error);
        }

        // The receipt is built from the content THIS delivery published, before
        // the ledger write moves it, so the commit can never seal a different
        // transaction's delivery of the same path. It is minted only when the
        // serving engine certifies it accepted exactly these bytes.
        let receipt = self
            .certified_delivery(tsx_path, prepared.prepared.clone())
            .map(|delivered| SyncedTsxSurface::from_delivered(tsx_path, delivered));
        self.record_delivered_carrier_surface(tsx_path, tsx_content, prepared.prepared);

        // When an installed package becomes available, publish the unrewritten
        // carrier first. Closing its old overlay before that update would break
        // the still-live previous carrier if the update failed.
        if virtual_path.is_none() {
            self.close_virtual_verter_types(tsx_path, lane).await?;
        }
        Ok(CarrierDelivery::Delivered(receipt))
    }

    /// [`Self::sync_tsx`] guarded by a delivery fence evaluated under the
    /// per-path delivery lock (see [`Self::publish_tsx_fenced`]). The answer
    /// carries the delivery's own receipt, so the caller's commit seals exactly
    /// the bytes this call published; [`CarrierDelivery::Refused`] means the
    /// fence refused and the provider received nothing.
    pub(crate) async fn sync_tsx_fenced(
        &self,
        tsx_path: &str,
        tsx_content: &str,
        fence: &(dyn Fn() -> bool + Sync),
    ) -> Result<CarrierDelivery, TypeProviderError> {
        self.publish_tsx_fenced(
            tsx_path,
            tsx_content,
            ProviderLane::Foreground,
            ProviderFileVerb::Update,
            Some(fence),
        )
        .await
    }

    /// [`Self::open_tsx`] guarded by a delivery fence evaluated under the
    /// per-path delivery lock (see [`Self::publish_tsx_fenced`]). The answer
    /// carries the delivery's own receipt, so the caller's commit seals exactly
    /// the bytes this call published; [`CarrierDelivery::Refused`] means the
    /// fence refused and the provider received nothing.
    pub(crate) async fn open_tsx_fenced(
        &self,
        tsx_path: &str,
        tsx_content: &str,
        fence: &(dyn Fn() -> bool + Sync),
    ) -> Result<CarrierDelivery, TypeProviderError> {
        self.publish_tsx_fenced(
            tsx_path,
            tsx_content,
            ProviderLane::Foreground,
            ProviderFileVerb::Open,
            Some(fence),
        )
        .await
    }

    pub(super) async fn close_tsx_in_lane(
        &self,
        tsx_path: &str,
        lane: ProviderLane,
    ) -> Result<(), TypeProviderError> {
        let lock = self.virtual_verter_types_lock(tsx_path);
        let _guard = lock.lock().await;
        // Our own record of why this carrier could not be prepared describes a
        // buffer we are giving up on, so it is dropped whatever the provider
        // says. Otherwise a first-open failure — which never commits provider
        // state and so may never reach the success branch below — would outlive
        // every buffer it ever described.
        self.clear_carrier_preparation_failure(tsx_path);
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
        // Cleanup derives the SAME descriptor-owned path creation derived
        // (`prepare_carrier_provider_surface`), so a naming change can never
        // strand a published overlay under a stale locally-formatted spelling.
        // A refusal (no basename) proves no overlay EXISTS to clean up: every
        // `virtual_verter_types_paths` entry was inserted from the SAME
        // deterministic derivation over the same `tsx_path`, so a path this
        // refuses to name was refused at publication too and never entered the
        // set — returning `Ok` skips nothing that lives.
        let Some(path) = verter_session::framework::descriptor::verter_types_sidecar_path(tsx_path)
        else {
            return Ok(());
        };
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
