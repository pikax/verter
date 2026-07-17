//! Component contract resolution helpers.
//!
//! Inherent-impl extension methods on [`super::VerterLanguageServer`]
//! covering import-specifier resolution, child-component context
//! building, barrel re-export following, and template-contract
//! definition resolution (props, events, v-model, slots).
//!
//! The sibling lives as a private child module under `server/mod.rs` so it sees
//! the parent's private struct fields without visibility widening.

use std::collections::HashSet;

use tower_lsp_server::ls_types::{GotoDefinitionResponse, Hover, Location, Position, Range, Uri};

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::uri_to_canonical_id;
use crate::features::hover;
use crate::type_provider::merge;

use super::child_prop_rename::ChildPropUsageClass;
use super::server_utils::{
    attr_name_match_rank, event_name_match_rank, extract_word_at_offset,
    goto_response_from_locations, is_default_export_component_carrier, listener_prop_candidates,
    location_from_span, push_unique_location, resolve_import_path, to_pascal_case,
};
use super::{ResolvedComponentDocument, VerterLanguageServer};

/// Build the canonical candidates for an imported component in semantic
/// authority order. The analysis snapshot records the resolution used to
/// compile the parent; a later workspace/provider state must not displace that
/// identity. Workspace resolution remains the recovery path for snapshots that
/// lack a resolved import, and lexical resolution is the final compatibility
/// fallback for not-yet-indexed relative files.
fn imported_component_canonical_candidates(
    parent_canonical_id: &str,
    parent_analysis: Option<&verter_session::FileAnalysisSnapshot>,
    import_source: &str,
    workspace_resolved: Option<String>,
) -> Vec<String> {
    let mut candidates = Vec::with_capacity(3);
    let mut push_unique = |candidate: String| {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    };

    if let Some(resolved) = parent_analysis
        .and_then(|analysis| {
            analysis
                .imports
                .iter()
                .find(|import| import.source == import_source)
        })
        .and_then(|import| import.resolved_canonical_id.clone())
    {
        push_unique(resolved);
    }

    if let Some(resolved) = workspace_resolved {
        push_unique(resolved);
    }

    let lexical = if import_source.starts_with('.') {
        let parent_dir = parent_canonical_id
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .unwrap_or(parent_canonical_id);
        resolve_import_path(parent_dir, import_source)
    } else {
        import_source.to_string()
    };
    push_unique(lexical);

    candidates
}

impl VerterLanguageServer {
    /// Ensure a resolved component carrier is present and its analysis-facing
    /// compile has run. `ensure_compiled` is content/profile cached, so this is
    /// cheap on warm hovers while still making unopened imported carriers
    /// reliable.
    fn ensure_component_ready(
        &self,
        canonical_id: &str,
    ) -> Option<verter_session::FileAnalysisSnapshot> {
        let host = self.documents.host();
        if host.get_source(canonical_id).is_none() && !host.ensure_loaded(canonical_id) {
            return None;
        }

        if is_default_export_component_carrier(canonical_id) {
            let profile = self.documents.tsx_profile.read().clone();
            // ANALYSIS-facing side effect: this is intentionally the shared
            // compile path, which also leaves the exact carrier public API ready
            // for the hover builder. Main-less Svelte carriers may return a
            // missing-main result after publishing analysis, so read the
            // authoritative analysis store regardless of the return value.
            let _ = host.ensure_compiled(canonical_id, &profile);
        }

        host.get_analysis(canonical_id)
    }

    fn imported_component_export_name<'a>(
        parent_analysis: Option<&'a verter_session::FileAnalysisSnapshot>,
        import_source: &str,
        local_binding_name: Option<&'a str>,
    ) -> Option<&'a str> {
        let local_binding_name = local_binding_name?;
        parent_analysis
            .and_then(|analysis| {
                analysis
                    .imports
                    .iter()
                    .find(|import| import.source == import_source)
            })
            .and_then(|import| {
                import
                    .bindings
                    .iter()
                    .find(|binding| binding.name == local_binding_name)
            })
            .and_then(|binding| binding.imported_name.as_deref())
            .or(Some(local_binding_name))
    }

    /// Resolve one imported component identity through the single canonical
    /// candidate path shared by tag hovers, component usages, and import
    /// bindings.
    fn resolve_imported_component_canonical_id(
        &self,
        parent_uri: &Uri,
        parent_analysis: Option<&verter_session::FileAnalysisSnapshot>,
        import_source: &str,
        local_binding_name: Option<&str>,
    ) -> Option<String> {
        let parent_canonical_id = uri_to_canonical_id(parent_uri);
        let workspace_resolved = self.resolve_import_specifier(&parent_canonical_id, import_source);
        let candidates = imported_component_canonical_candidates(
            &parent_canonical_id,
            parent_analysis,
            import_source,
            workspace_resolved,
        );
        let export_name = Self::imported_component_export_name(
            parent_analysis,
            import_source,
            local_binding_name,
        );

        for candidate in candidates {
            if is_default_export_component_carrier(&candidate) {
                if self.ensure_component_ready(&candidate).is_some() {
                    return Some(candidate);
                }
                continue;
            }

            // Barrel export surfaces are populated by the shared host load.
            if self.documents.host().get_source(&candidate).is_none()
                && !self.documents.host().ensure_loaded(&candidate)
            {
                continue;
            }
            let Some(export_name) = export_name else {
                continue;
            };
            let Some((resolved_id, _, _)) = self
                .documents
                .host()
                .get_export_span_follow_reexports(&candidate, export_name)
            else {
                continue;
            };
            if is_default_export_component_carrier(&resolved_id)
                && self.ensure_component_ready(&resolved_id).is_some()
            {
                return Some(resolved_id);
            }
        }

        None
    }

    fn resolved_component_document(
        &self,
        child_canonical_id: &str,
    ) -> Option<ResolvedComponentDocument> {
        let child_analysis = self
            .documents
            .host()
            .get_analysis(child_canonical_id)
            .or_else(|| self.ensure_component_ready(child_canonical_id))?;
        let child_source = self.documents.host().get_source(child_canonical_id)?;
        let child_line_index = LineIndex::new(&child_source, self.documents.encoding());
        let child_uri = crate::uri::path_to_file_uri(child_canonical_id)?;

        Some(ResolvedComponentDocument {
            uri: child_uri,
            analysis: child_analysis,
            line_index: child_line_index,
        })
    }

    pub(super) fn resolve_import_specifier(
        &self,
        parent_canonical_id: &str,
        specifier: &str,
    ) -> Option<String> {
        self.documents
            .host()
            .resolve_import_via_workspace(parent_canonical_id, specifier)
    }

    pub(super) fn component_import_binding_name(
        &self,
        analysis: &verter_session::FileAnalysisSnapshot,
        component: &verter_semantic::analysis::template::TemplateComponentUsage,
    ) -> Option<String> {
        let import_source = component.import_source.as_ref()?;
        let import = analysis
            .imports
            .iter()
            .find(|import| import.source == *import_source)?;

        import
            .bindings
            .iter()
            .find(|binding| {
                binding.name == component.name || to_pascal_case(&binding.name) == component.name
            })
            .map(|binding| binding.name.clone())
            .or_else(|| import.bindings.first().map(|binding| binding.name.clone()))
            .or_else(|| Some("default".to_string()))
    }

    pub(super) fn resolve_component_document_for_usage(
        &self,
        parent_uri: &Uri,
        parent_analysis: &verter_session::FileAnalysisSnapshot,
        component: &verter_semantic::analysis::template::TemplateComponentUsage,
    ) -> Option<ResolvedComponentDocument> {
        let import_source = component.import_source.as_ref()?;
        let binding_name = self.component_import_binding_name(parent_analysis, component);
        let child_canonical_id = self.resolve_imported_component_canonical_id(
            parent_uri,
            Some(parent_analysis),
            import_source,
            binding_name.as_deref(),
        )?;

        self.resolved_component_document(&child_canonical_id)
    }

    pub(super) fn resolve_component_document_for_import_binding(
        &self,
        parent_uri: &Uri,
        parent_analysis: &verter_session::FileAnalysisSnapshot,
        import_source: &str,
        binding_name: &str,
    ) -> Option<ResolvedComponentDocument> {
        let child_canonical_id = self.resolve_imported_component_canonical_id(
            parent_uri,
            Some(parent_analysis),
            import_source,
            Some(binding_name),
        )?;

        self.resolved_component_document(&child_canonical_id)
    }

    pub(super) fn collect_component_event_definition_locations(
        &self,
        child: &ResolvedComponentDocument,
        event_name: &str,
    ) -> Vec<Location> {
        let mut locations = Vec::new();
        let mut seen = HashSet::new();

        let mut emit_locations = Vec::new();
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineEmits {
                continue;
            }
            for emit_field in &mac.emit_fields {
                if let Some(rank) = event_name_match_rank(event_name, &emit_field.name) {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, emit_field.span)
                    {
                        emit_locations.push((rank, location));
                    }
                }
            }
        }
        if let Some(template) = child.analysis.template.as_ref() {
            for emit in &template.emit_definitions {
                if !emit.is_declared {
                    continue;
                }
                if let Some(rank) = event_name_match_rank(event_name, &emit.event_name) {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, emit.span)
                    {
                        emit_locations.push((rank, location));
                    }
                }
            }
        }
        emit_locations.sort_by_key(|(rank, location)| {
            (
                *rank,
                location.range.start.line,
                location.range.start.character,
                location.range.end.line,
                location.range.end.character,
            )
        });
        for (_, location) in emit_locations {
            push_unique_location(&mut locations, &mut seen, location);
        }

        let prop_candidates = listener_prop_candidates(event_name);
        let mut prop_locations = Vec::new();
        for mac in child.analysis.macros.iter() {
            for prop_field in &mac.prop_fields {
                if let Some(rank) = prop_candidates
                    .iter()
                    .position(|candidate| candidate == &prop_field.name)
                {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, prop_field.span)
                    {
                        prop_locations.push((rank, location));
                    }
                }
            }
        }
        if let Some(template) = child.analysis.template.as_ref() {
            for prop_definition in &template.prop_definitions {
                if let Some(rank) = prop_candidates
                    .iter()
                    .position(|candidate| candidate == &prop_definition.name)
                {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, prop_definition.span)
                    {
                        prop_locations.push((rank, location));
                    }
                }
            }
        }
        prop_locations.sort_by_key(|(rank, location)| {
            (
                *rank,
                location.range.start.line,
                location.range.start.character,
                location.range.end.line,
                location.range.end.character,
            )
        });
        for (_, location) in prop_locations {
            push_unique_location(&mut locations, &mut seen, location);
        }

        locations
    }

    pub(super) fn resolve_definition_path(
        &self,
        canonical_id: &str,
        specifier: &str,
    ) -> Option<String> {
        self.resolve_import_specifier(canonical_id, specifier)
    }

    pub(super) fn resolve_precise_export_location(
        &self,
        target_canonical_id: &str,
        binding_name: &str,
    ) -> Option<Location> {
        let host = &self.documents.host;
        let (resolved_id, start, end) = host
            .get_export_span_follow_reexports(target_canonical_id, binding_name)
            .or_else(|| {
                let (s, e) = host.get_export_span(target_canonical_id, binding_name)?;
                Some((target_canonical_id.to_string(), s, e))
            })?;
        let target_source = host.get_source(&resolved_id)?;
        let target_li = LineIndex::new(&target_source, self.position_encoding.read().clone());
        let start_pos = target_li.offset_to_position(start)?;
        let end_pos = target_li.offset_to_position(end)?;
        Some(Location {
            uri: merge::file_path_to_uri(&resolved_id)?,
            range: Range {
                start: start_pos,
                end: end_pos,
            },
        })
    }

    pub(super) fn resolve_template_identifier(
        &self,
        uri: &Uri,
        analysis: &verter_session::FileAnalysisSnapshot,
        line_index: &LineIndex,
        word: &str,
    ) -> Option<GotoDefinitionResponse> {
        for import in &analysis.imports {
            for binding in &import.bindings {
                if binding.name != word {
                    continue;
                }

                if let Some(canonical_id) = import.resolved_canonical_id.as_deref() {
                    if let Some(location) =
                        self.resolve_precise_export_location(canonical_id, &binding.name)
                    {
                        return Some(GotoDefinitionResponse::Scalar(location));
                    }
                    if is_default_export_component_carrier(canonical_id) {
                        if let Some(location) =
                            self.resolve_precise_export_location(canonical_id, "default")
                        {
                            return Some(GotoDefinitionResponse::Scalar(location));
                        }
                    }
                }

                if let Some(resolved) =
                    self.resolve_definition_path(&uri_to_canonical_id(uri), &import.source)
                {
                    if let Some(location) =
                        self.resolve_precise_export_location(&resolved, &binding.name)
                    {
                        return Some(GotoDefinitionResponse::Scalar(location));
                    }
                    if is_default_export_component_carrier(&resolved) {
                        if let Some(location) =
                            self.resolve_precise_export_location(&resolved, "default")
                        {
                            return Some(GotoDefinitionResponse::Scalar(location));
                        }
                    }
                }

                if let Some(location) = location_from_span(uri, line_index, binding.span) {
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }

        if let Some(binding) = analysis
            .bindings
            .iter()
            .find(|binding| binding.name == word)
        {
            if let Some(location) = location_from_span(uri, line_index, binding.span) {
                return Some(GotoDefinitionResponse::Scalar(location));
            }
        }

        for mac in analysis.macros.iter() {
            if let Some(prop_field) = mac.prop_fields.iter().find(|field| field.name == word) {
                if let Some(location) = location_from_span(uri, line_index, prop_field.span) {
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }

            if mac.binding_name.as_deref() == Some(word) {
                if let Some(location) = location_from_span(uri, line_index, mac.span) {
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }

        None
    }

    /// Unified component contract resolution: props, events, v-model, slots.
    /// Runs BEFORE `definition_at_position` and returns `Some` if any contract
    /// surface was hit, or `None` to fall through to normal definition logic.
    pub(super) fn try_component_contract_definition(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> Option<GotoDefinitionResponse> {
        let doc = self.documents.get(uri)?;
        let analysis = self.documents.get_analysis(uri)?;
        let template = analysis.template.as_ref()?;
        let offset = doc.line_index.position_to_offset(position)?;

        // ── Props (cursor on a `<Child prop=…>` prop NAME) ──────────────────
        // SHARED resolution with cross-file rename: component match + prop
        // `name_span` containment + child resolution come from the one
        // `resolve_child_prop_usage_at_cursor`, so the two paths cannot drift. The
        // navigation conveniences below (shorthand parent binding, template-level
        // prop definitions, the final navigate-to-child-file fallback) stay
        // goto-definition-ONLY — they are NOT safe rename declarations, so rename
        // must not inherit them. A prop-name cursor is mutually exclusive with the
        // directive cursors the events/v-model/slot loop handles.
        if let ChildPropUsageClass::Resolved(resolved) =
            self.resolve_child_prop_usage_at_cursor(uri, position)
        {
            let mut locations = Vec::new();

            // For shorthand props, also resolve the parent binding.
            if resolved.usage.parent_is_shorthand {
                if let Some(parent_def) = self.resolve_template_identifier(
                    uri,
                    &analysis,
                    &doc.line_index,
                    &resolved.usage.parent_prop_name,
                ) {
                    match parent_def {
                        GotoDefinitionResponse::Scalar(loc) => locations.push(loc),
                        GotoDefinitionResponse::Array(locs) => locations.extend(locs),
                        GotoDefinitionResponse::Link(links) => {
                            locations.extend(links.into_iter().map(|link| Location {
                                uri: link.target_uri,
                                range: link.target_selection_range,
                            }));
                        }
                    }
                }
            }

            // Find matching prop field in child's defineProps (the safe rename decl).
            // Kebab ↔ camel equivalence so `:my-prop` lands on `myProp`.
            if let Some(decl_span) = self.resolve_child_macro_prop_declaration(&resolved) {
                if let Some(loc) =
                    location_from_span(&resolved.child.uri, &resolved.child.line_index, decl_span)
                {
                    locations.push(loc);
                }
            } else if let Some(child_template) = resolved.child.analysis.template.as_ref() {
                // Fallback: template-level prop definitions (navigation convenience).
                let requested = resolved.usage.parent_prop_name.as_str();
                let mut best: Option<(u8, verter_span::Span)> = None;
                for prop_def in &child_template.prop_definitions {
                    if let Some(rank) = attr_name_match_rank(requested, &prop_def.name) {
                        let better = match best {
                            None => true,
                            Some((best_rank, best_span)) => {
                                rank < best_rank
                                    || (rank == best_rank
                                        && (prop_def.span.start, prop_def.span.end)
                                            < (best_span.start, best_span.end))
                            }
                        };
                        if better {
                            best = Some((rank, prop_def.span));
                        }
                    }
                }
                if let Some((_, span)) = best {
                    if let Some(loc) =
                        location_from_span(&resolved.child.uri, &resolved.child.line_index, span)
                    {
                        locations.push(loc);
                    }
                }
            }
            // Fail closed when the child has no matching prop declaration.
            // A file-start fallback would paint a Ctrl+hover underline for
            // undeclared attribute names (mis-mapped affordance). Shorthand
            // may still surface the parent binding alone.
            if !locations.is_empty() {
                return Some(goto_response_from_locations(locations));
            }
            // Cursor was on a prop name for a resolved child; do not fall through
            // to generic word navigation / TypeProvider on the same token.
            return None;
        }

        for element in &template.elements {
            if !element.is_component && element.tag != "template" {
                continue;
            }

            // For <template #slot> elements, find the parent component. The props
            // hit is handled BEFORE this loop via the shared usage resolver; this
            // loop resolves only directive cursors (events / v-model / slots), so it
            // needs only the resolved child component document.
            let child = if element.tag == "template" {
                // Walk up to the parent element to find the component
                let parent_idx = match element.parent_index {
                    Some(idx) => idx as usize,
                    None => continue,
                };
                let parent_element = match template.elements.get(parent_idx) {
                    Some(element) => element,
                    None => continue,
                };
                if !parent_element.is_component {
                    continue;
                }
                let comp = match template.components.iter().find(|c| {
                    offset >= c.span.start
                        && offset < c.span.end
                        && (c.name == parent_element.tag
                            || c.name == to_pascal_case(&parent_element.tag))
                }) {
                    Some(component) => component,
                    None => continue,
                };
                match self.resolve_component_document_for_usage(uri, &analysis, comp) {
                    Some(child) => child,
                    None => continue,
                }
            } else {
                let comp = template.components.iter().find(|c| {
                    offset >= c.span.start
                        && offset < c.span.end
                        && (c.name == element.tag || c.name == to_pascal_case(&element.tag))
                });
                let comp = match comp {
                    Some(c) => c,
                    None => continue,
                };
                match self.resolve_component_document_for_usage(uri, &analysis, comp) {
                    Some(c) => c,
                    None => continue,
                }
            };

            // ── Events (v-on) ───────────────────────────────────────
            for directive in &element.directives {
                if directive.name == "on" {
                    if let Some(arg_span) = directive.arg_span {
                        if offset >= arg_span.start && offset < arg_span.end {
                            let event_name = directive.argument.as_deref()?;
                            let locations = self
                                .collect_component_event_definition_locations(&child, event_name);
                            return if locations.is_empty() {
                                None
                            } else {
                                Some(goto_response_from_locations(locations))
                            };
                        }
                    }
                }
            }

            // ── V-model ─────────────────────────────────────────────
            for directive in &element.directives {
                if directive.name != "model" {
                    continue;
                }

                // Named v-model: `v-model:title="t"` — cursor on "title" (the arg)
                if let Some(arg_span) = directive.arg_span {
                    if offset >= arg_span.start && offset < arg_span.end {
                        let model_name = directive.argument.as_deref().unwrap_or("modelValue");
                        return self.resolve_vmodel_definition(&child, model_name);
                    }
                }

                // Plain v-model: `v-model="val"` — cursor on the directive name ("v-model")
                // The name area spans from directive.span.start up to name_end
                if directive.argument.is_none()
                    && offset >= directive.span.start
                    && offset < directive.name_end
                {
                    return self.resolve_vmodel_definition(&child, "modelValue");
                }
            }

            // ── Slot name (v-slot / #) ──────────────────────────────
            for directive in &element.directives {
                if directive.name != "slot" {
                    continue;
                }

                // Slot name: cursor on arg_span (#header → "header")
                if let Some(arg_span) = directive.arg_span {
                    if offset >= arg_span.start && offset < arg_span.end {
                        let slot_name = directive.argument.as_deref().unwrap_or("default");
                        return self.resolve_slot_name_definition(&child, slot_name);
                    }
                }

                // Slot-prop binding: cursor inside expression_span (#default="{ item }")
                if let Some(expr_span) = directive.expression_span {
                    if offset >= expr_span.start && offset < expr_span.end {
                        let slot_name = directive.argument.as_deref().unwrap_or("default");
                        // Find the word under cursor
                        let source_bytes = doc.source.as_bytes();
                        let word = extract_word_at_offset(source_bytes, offset, expr_span);
                        if let Some(word) = word {
                            return self.resolve_slot_binding_definition(&child, slot_name, &word);
                        }
                    }
                }
            }
        }

        None
    }

    /// Resolve barrel-file export clicks to terminal target.
    ///
    /// When the cursor is on an `ExportSignature` that is a re-export
    /// (has `reexport_source`), follow the chain to the terminal declaration.
    pub(super) fn try_barrel_export_definition(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> Option<GotoDefinitionResponse> {
        let doc = self.documents.get(uri)?;
        let analysis = self.documents.get_analysis(uri)?;
        let offset = doc.line_index.position_to_offset(position)?;

        let encoding = self.position_encoding.read().clone();
        let host = &self.documents.host;
        let canonical_id = uri_to_canonical_id(uri);

        for sig in analysis.export_signatures.iter() {
            // Only handle re-exports (has a source module)
            if sig.reexport_source.is_none() {
                continue;
            }

            // Check if cursor is on the exported name span
            let on_exported = offset >= sig.span.start && offset < sig.span.end;

            // Check if cursor is on the local name span (for aliased re-exports)
            let on_local = sig
                .local_span
                .as_ref()
                .is_some_and(|ls| offset >= ls.start && offset < ls.end);

            if !on_exported && !on_local {
                continue;
            }

            // Determine the binding name to follow in the target module
            let binding_to_follow = if on_local {
                // Clicking on local side (e.g., `default` in `export { default as Popup }`)
                // Follow this local name in the target
                sig.reexport_local.as_deref().unwrap_or(sig.name.as_str())
            } else {
                // Clicking on exported side (e.g., `Overlay` in `export { default as Overlay }`)
                // The name exported from this file; follow via get_export_span_follow_reexports
                sig.name.as_str()
            };

            // Follow the re-export chain to the terminal
            let terminal = if on_local {
                // For local side, resolve the source module first, then follow
                let resolved = host.resolve_import(&canonical_id, sig.reexport_source.as_ref()?)?;
                let local_name = sig.reexport_local.as_deref().unwrap_or(sig.name.as_str());
                host.get_export_span_follow_reexports(&resolved, local_name)
            } else {
                host.get_export_span_follow_reexports(&canonical_id, binding_to_follow)
            };

            if let Some((resolved_id, start, end)) = terminal {
                let target_source = host.get_source(&resolved_id)?;
                let target_li = LineIndex::new(&target_source, encoding);
                let start_pos = target_li.offset_to_position(start)?;
                let end_pos = target_li.offset_to_position(end)?;
                let target_uri = merge::file_path_to_uri(&resolved_id)?;
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: target_uri,
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                }));
            }
        }

        None
    }

    /// Canonicalize a raw type-provider path into the shared canonical-ID form
    /// before keying `host.get_analysis`. Delegates to the single owner
    /// (`verter_span::path`) so this lookup keys byte-identically to every other
    /// producer — the previous hand-rolled form omitted the `//?/` extended-
    /// prefix strip and the trailing-slash strip, so a Windows extended-prefix
    /// path missed the analysis cache. The leading `trim()` is retained because
    /// provider locations can carry surrounding whitespace.
    pub(super) fn canonicalize_provider_path(path: &str) -> String {
        verter_span::path::canonicalize_path(path.trim())
    }

    /// Resolve a raw type-provider location that lands on a barrel file to the terminal target.
    pub(super) fn resolve_barrel_type_provider_location(
        &self,
        path: &str,
        start: u32,
        end: u32,
    ) -> Option<Location> {
        let canonical = Self::canonicalize_provider_path(path);
        let host = &self.documents.host;
        let analysis = host.get_analysis(&canonical)?;
        let (sig, matched_local) = analysis.export_signatures.iter().find_map(|sig| {
            sig.reexport_source.as_ref()?;
            if sig.span.start <= start && end <= sig.span.end {
                return Some((sig, false));
            }
            if let Some(local_span) = sig.local_span.as_ref() {
                if local_span.start <= start && end <= local_span.end {
                    return Some((sig, true));
                }
            }
            None
        })?;

        let (terminal_id, terminal_start, terminal_end) = if matched_local {
            let target = host.resolve_import(&canonical, sig.reexport_source.as_ref()?)?;
            let binding = sig.reexport_local.as_deref().unwrap_or(sig.name.as_str());
            host.get_export_span_follow_reexports(&target, binding)?
        } else {
            host.get_export_span_follow_reexports(&canonical, &sig.name)?
        };

        let source = host.get_source(&terminal_id)?;
        let line_index = LineIndex::new(&source, self.position_encoding.read().clone());
        let start_pos = line_index.offset_to_position(terminal_start)?;
        let end_pos = line_index.offset_to_position(terminal_end)?;
        let uri = merge::file_path_to_uri(&terminal_id)?;
        Some(Location {
            uri,
            range: Range {
                start: start_pos,
                end: end_pos,
            },
        })
    }

    /// Resolve type provider definition results through barrel re-exports.
    ///
    /// When the type provider returns a location in a barrel file (`.ts`/`.js` with
    /// re-exports), resolve each location to the terminal declaration so the user
    /// doesn't land in the barrel file.
    pub(super) fn resolve_barrel_locations(
        &self,
        response: Option<GotoDefinitionResponse>,
    ) -> Option<GotoDefinitionResponse> {
        let response = response?;
        let encoding = self.position_encoding.read().clone();
        let host = &self.documents.host;

        let resolve_location = |loc: Location| -> Location {
            let canonical = uri_to_canonical_id(&loc.uri);
            // Check if this file has re-export signatures at the target position
            if let Some(analysis) = host.get_analysis(&canonical) {
                // Find which export signature the target position falls within
                if let Some(source) = host.get_source(&canonical) {
                    let target_li = LineIndex::new(&source, encoding.clone());
                    if let Some(offset) = target_li.position_to_offset(&loc.range.start) {
                        for sig in analysis.export_signatures.iter() {
                            if sig.reexport_source.is_none() {
                                continue;
                            }
                            let on_sig = offset >= sig.span.start && offset < sig.span.end;
                            let on_local = sig
                                .local_span
                                .as_ref()
                                .is_some_and(|ls| offset >= ls.start && offset < ls.end);
                            if !on_sig && !on_local {
                                continue;
                            }
                            // Follow to terminal
                            if let Some(end_offset) = target_li.position_to_offset(&loc.range.end) {
                                if let Some(resolved) = self.resolve_barrel_type_provider_location(
                                    &canonical, offset, end_offset,
                                ) {
                                    return resolved;
                                }
                            }
                            break;
                        }
                    }
                }
            }
            loc
        };

        Some(match response {
            GotoDefinitionResponse::Scalar(loc) => {
                GotoDefinitionResponse::Scalar(resolve_location(loc))
            }
            GotoDefinitionResponse::Array(locs) => {
                GotoDefinitionResponse::Array(locs.into_iter().map(resolve_location).collect())
            }
            other => other,
        })
    }

    /// Resolve v-model to child defineModel (Tier 1), then classic prop+emit (Tier 2),
    /// then template-level definitions (Tier 3).
    pub(super) fn resolve_vmodel_definition(
        &self,
        child: &ResolvedComponentDocument,
        model_name: &str,
    ) -> Option<GotoDefinitionResponse> {
        let mut locations = Vec::new();

        // Tier 1: defineModel macro
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineModel {
                continue;
            }
            let macro_model_name = mac.model_name.as_deref().unwrap_or("modelValue");
            if macro_model_name == model_name {
                if let Some(loc) = location_from_span(&child.uri, &child.line_index, mac.span) {
                    locations.push(loc);
                }
            }
        }

        if !locations.is_empty() {
            return Some(goto_response_from_locations(locations));
        }

        // Tier 2: classic prop + emit pattern
        // The prop is the model_name itself (e.g., "modelValue" or "title")
        for mac in child.analysis.macros.iter() {
            if let Some(prop_field) = mac.prop_fields.iter().find(|f| f.name == model_name) {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, prop_field.span)
                {
                    locations.push(loc);
                }
            }
            // The emit is `update:modelName`
            let emit_name = format!("update:{model_name}");
            if let Some(emit_field) = mac.emit_fields.iter().find(|f| f.name == emit_name) {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, emit_field.span)
                {
                    locations.push(loc);
                }
            }
        }

        if !locations.is_empty() {
            return Some(goto_response_from_locations(locations));
        }

        // Tier 3: template-level definitions
        if let Some(child_template) = child.analysis.template.as_ref() {
            if let Some(prop_def) = child_template
                .prop_definitions
                .iter()
                .find(|d| d.name == model_name)
            {
                if let Some(loc) = location_from_span(&child.uri, &child.line_index, prop_def.span)
                {
                    locations.push(loc);
                }
            }
        }

        if !locations.is_empty() {
            return Some(goto_response_from_locations(locations));
        }

        None
    }

    /// Resolve a slot name (#header) to the child's defineSlots field or template DefinedSlot.
    /// Kebab ↔ camel equivalence matches Vue's HTML-case contract
    /// (`#my-slot` → `mySlot` / `my-slot`).
    pub(super) fn resolve_slot_name_definition(
        &self,
        child: &ResolvedComponentDocument,
        slot_name: &str,
    ) -> Option<GotoDefinitionResponse> {
        // Check defineSlots macro first (exact rank preferred over case-fold).
        let mut best: Option<(u8, verter_span::Span)> = None;
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineSlots {
                continue;
            }
            for slot_field in &mac.slot_fields {
                if let Some(rank) = attr_name_match_rank(slot_name, &slot_field.name) {
                    let better = match best {
                        None => true,
                        Some((best_rank, best_span)) => {
                            rank < best_rank
                                || (rank == best_rank
                                    && (slot_field.span.start, slot_field.span.end)
                                        < (best_span.start, best_span.end))
                        }
                    };
                    if better {
                        best = Some((rank, slot_field.span));
                    }
                }
            }
        }
        if let Some((_, span)) = best {
            if let Some(loc) = location_from_span(&child.uri, &child.line_index, span) {
                return Some(GotoDefinitionResponse::Scalar(loc));
            }
        }

        // Fallback: template-level DefinedSlot
        if let Some(child_template) = child.analysis.template.as_ref() {
            let mut best: Option<(u8, verter_span::Span)> = None;
            for defined_slot in &child_template.defined_slots {
                if let Some(rank) = attr_name_match_rank(slot_name, &defined_slot.name) {
                    let better = match best {
                        None => true,
                        Some((best_rank, best_span)) => {
                            rank < best_rank
                                || (rank == best_rank
                                    && (defined_slot.span.start, defined_slot.span.end)
                                        < (best_span.start, best_span.end))
                        }
                    };
                    if better {
                        best = Some((rank, defined_slot.span));
                    }
                }
            }
            if let Some((_, span)) = best {
                if let Some(loc) = location_from_span(&child.uri, &child.line_index, span) {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }

        None
    }

    /// Resolve a slot-prop binding (e.g., "item" in `#default="{ item }"`) to
    /// the child's defineSlots binding span.
    pub(super) fn resolve_slot_binding_definition(
        &self,
        child: &ResolvedComponentDocument,
        slot_name: &str,
        binding_name: &str,
    ) -> Option<GotoDefinitionResponse> {
        // Check defineSlots macro — slot name uses kebab/camel equivalence;
        // the binding identifier itself is an exact TS identifier match.
        let mut best_slot: Option<(u8, &verter_semantic::analysis::AnalyzedSlotField)> = None;
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineSlots {
                continue;
            }
            for slot_field in &mac.slot_fields {
                if let Some(rank) = attr_name_match_rank(slot_name, &slot_field.name) {
                    let better = match best_slot {
                        None => true,
                        Some((best_rank, best_field)) => {
                            rank < best_rank
                                || (rank == best_rank
                                    && (slot_field.span.start, slot_field.span.end)
                                        < (best_field.span.start, best_field.span.end))
                        }
                    };
                    if better {
                        best_slot = Some((rank, slot_field));
                    }
                }
            }
        }
        if let Some((_, slot_field)) = best_slot {
            if let Some(binding) = slot_field.bindings.iter().find(|b| b.name == binding_name) {
                if binding.span.start != 0 || binding.span.end != 0 {
                    if let Some(loc) =
                        location_from_span(&child.uri, &child.line_index, binding.span)
                    {
                        return Some(GotoDefinitionResponse::Scalar(loc));
                    }
                }
            }
        }

        None
    }

    /// Resolve a child component with full context for cross-file editing.
    ///
    /// The parent's compiled import identity is authoritative. Workspace and
    /// lexical candidates are recovery paths only; barrel traversal resolves to
    /// the terminal framework carrier before source or analysis is read.
    pub(super) fn resolve_component_context(
        &self,
        parent_uri: &Uri,
        import_source: &str,
        component_name: Option<&str>,
    ) -> Option<crate::features::cross_file::ChildComponentContext> {
        let parent_analysis = self.documents.get_analysis(parent_uri);
        let child_canonical_id = self.resolve_imported_component_canonical_id(
            parent_uri,
            parent_analysis.as_ref(),
            import_source,
            component_name,
        )?;

        let analysis = self
            .documents
            .host()
            .get_analysis(&child_canonical_id)
            .or_else(|| self.ensure_component_ready(&child_canonical_id))?;

        // Get the child's source
        let child_source_arc = self.documents.host().get_source(&child_canonical_id)?;
        let child_source = child_source_arc.to_string();
        let child_uri = crate::uri::path_to_file_uri(&child_canonical_id)?;
        let blocks = scan_sfc_blocks(&child_source);
        let line_index = LineIndex::new(&child_source, self.documents.encoding());

        Some(crate::features::cross_file::ChildComponentContext {
            canonical_id: child_canonical_id,
            uri: child_uri,
            source: child_source,
            analysis,
            blocks,
            line_index,
        })
    }

    pub(super) fn child_hover_for_target(
        &self,
        parent_uri: &Uri,
        target: &hover::ChildHoverTarget,
    ) -> Option<Hover> {
        match target {
            hover::ChildHoverTarget::ComponentTag(target) => {
                let child = self.resolve_component_context(
                    parent_uri,
                    &target.import_source,
                    Some(&target.component_name),
                )?;
                let projection = self
                    .documents
                    .host()
                    .get_public_api_projection(&child.canonical_id);
                Some(hover::build_child_component_hover(
                    &target.component_name,
                    &target.import_source,
                    &child.analysis,
                    projection
                        .as_ref()
                        .and_then(|projection| projection.contract.as_ref()),
                    projection
                        .as_ref()
                        .map(|projection| projection.response.code.as_ref()),
                    &target.usage_props,
                ))
            }
            hover::ChildHoverTarget::ImportBinding(target) => {
                let parent_analysis = self.documents.get_analysis(parent_uri)?;
                let child = self.resolve_component_document_for_import_binding(
                    parent_uri,
                    &parent_analysis,
                    &target.import_source,
                    &target.binding_name,
                )?;
                let projection = self
                    .documents
                    .host()
                    .get_public_api_projection(&crate::documents::uri_to_canonical_id(&child.uri));
                Some(hover::build_child_component_hover(
                    &target.binding_name,
                    &target.import_source,
                    &child.analysis,
                    projection
                        .as_ref()
                        .and_then(|projection| projection.contract.as_ref()),
                    projection
                        .as_ref()
                        .map(|projection| projection.response.code.as_ref()),
                    &[],
                ))
            }
            hover::ChildHoverTarget::EventAttribute(target) => {
                let child =
                    self.resolve_component_context(parent_uri, &target.import_source, None)?;
                let public_api = self
                    .documents
                    .host()
                    .get_public_api(&child.canonical_id)
                    .map(|api| api.code.to_string());
                hover::build_child_event_hover(
                    &target.vue_attr,
                    &child.analysis,
                    public_api.as_deref(),
                )
            }
        }
    }
}

#[cfg(test)]
mod canonicalize_provider_path_tests {
    use super::VerterLanguageServer;

    #[test]
    fn delegates_to_owner_strips_extended_prefix_and_trailing_slash() {
        // The hand-rolled body previously omitted the `//?/` extended-prefix strip
        // and the trailing-slash strip, so a Windows extended-prefix path keyed
        // `get_analysis` under `//?/D:/x/` and missed the cache. Delegating to
        // the owner produces the same `d:/x` every other producer keys on.
        assert_eq!(
            VerterLanguageServer::canonicalize_provider_path("  //?/D:/x/  "),
            "d:/x"
        );
        assert_ne!(
            VerterLanguageServer::canonicalize_provider_path("//?/D:/x/"),
            "//?/D:/x/"
        );
        // UNC extended prefix and backslash + drive-lower still handled.
        assert_eq!(
            VerterLanguageServer::canonicalize_provider_path("\\\\?\\UNC\\srv\\sh"),
            "//srv/sh"
        );
        // Plain Unix path passes through unchanged (trim still applied).
        assert_eq!(
            VerterLanguageServer::canonicalize_provider_path(" /a/b/c.ts "),
            "/a/b/c.ts"
        );
    }
}

#[cfg(test)]
mod imported_component_candidate_tests {
    use super::imported_component_canonical_candidates;
    use verter_semantic::analysis::AnalyzedImport;

    // @ai-generated - Verifies parent-analysis identity outranks mutable fallbacks.
    #[test]
    fn analysis_identity_precedes_competing_workspace_and_lexical_fallbacks() {
        let mut analysis = verter_session::FileAnalysisSnapshot::default();
        analysis.imports.push(AnalyzedImport {
            source: "../shared/DirectChild".to_string(),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::default(),
            resolved_canonical_id: Some("/workspace/src/shared/DirectChild.vue".to_string()),
        });

        let candidates = imported_component_canonical_candidates(
            "/workspace/src/feature/DirectParent.vue",
            Some(&analysis),
            "../shared/DirectChild",
            Some("/workspace/src/shared/DirectChild.vue.tsx".to_string()),
        );

        assert_eq!(
            candidates,
            vec![
                "/workspace/src/shared/DirectChild.vue".to_string(),
                "/workspace/src/shared/DirectChild.vue.tsx".to_string(),
                "/workspace/src/shared/DirectChild".to_string(),
            ],
            "the analysis-owned canonical import identity must survive later provider/workspace state",
        );
    }
}
