//! Component contract resolution helpers.
//!
//! Inherent-impl extension methods on [`super::VerterLanguageServer`]
//! covering import-specifier resolution, child-component context
//! building, barrel re-export following, and template-contract
//! definition resolution (props, events, v-model, slots).
//!
//! All methods were moved verbatim from `server.rs` (now `server/mod.rs`)
//! lines 1641-2515 + the trio (resolve_component, resolve_component_context,
//! child_hover_for_target). No behaviour change. The sibling lives as a
//! private child module under `server/mod.rs` so it sees the parent's
//! private struct fields without visibility widening.

use std::collections::HashSet;

use tower_lsp_server::ls_types::{GotoDefinitionResponse, Hover, Location, Position, Range, Uri};

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::uri_to_canonical_id;
use crate::features::hover;
use crate::type_provider::merge;

use super::server_utils::{
    event_name_match_rank, extract_word_at_offset, goto_response_from_locations,
    is_default_export_component_carrier, listener_prop_candidates, location_from_span,
    push_unique_location, resolve_component_for, resolve_import_path, to_pascal_case,
};
use super::{ResolvedComponentDocument, VerterLanguageServer};

/// The resolved identity of a `<Child prop=…>` rename target: the child's
/// `{carrier}.ts` PUBLIC-API provider path, the prop name, and the prop's `.vue`
/// declaration span. Produced by [`VerterLanguageServer::resolve_child_prop_rename_target`]
/// to drive provider-agnostic synthesis of a cross-file Vue-prop rename's
/// child-declaration leg.
pub(super) struct ChildPropRenameTarget {
    /// The child component's `{carrier}.ts` PUBLIC-API provider path (the surface
    /// the cross-file prop rename's declaration edit lands against).
    pub(super) child_carrier_api_path: String,
    /// The prop name being renamed.
    pub(super) prop_name: String,
    /// The prop's `.vue` declaration span (file-absolute `.vue` bytes) — the typed
    /// identity the API generator keys its source-map token on.
    pub(super) prop_decl_span: verter_span::Span,
}

impl VerterLanguageServer {
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
        let parent_canonical_id = uri_to_canonical_id(parent_uri);
        let binding_name = self.component_import_binding_name(parent_analysis, component);
        let import = parent_analysis
            .imports
            .iter()
            .find(|import| import.source == *import_source);
        let mut resolved_targets = Vec::new();
        if let Some(resolved) = import.and_then(|entry| entry.resolved_canonical_id.clone()) {
            resolved_targets.push(resolved);
        }
        if let Some(resolved) = self.resolve_import_specifier(&parent_canonical_id, import_source) {
            if !resolved_targets
                .iter()
                .any(|candidate| candidate == &resolved)
            {
                resolved_targets.push(resolved);
            }
        }

        let child_canonical_id = resolved_targets.into_iter().find_map(|resolved_target| {
            if is_default_export_component_carrier(&resolved_target) {
                return Some(resolved_target);
            }

            binding_name.as_deref().and_then(|binding| {
                self.documents
                    .host()
                    .get_export_span_follow_reexports(&resolved_target, binding)
                    .map(|(resolved_id, _, _)| resolved_id)
                    .filter(|resolved_id| is_default_export_component_carrier(resolved_id))
            })
        })?;

        let child_analysis = self.documents.host().get_analysis(&child_canonical_id)?;
        let child_source = self.documents.host().get_source(&child_canonical_id)?;
        let child_line_index = LineIndex::new(&child_source, self.documents.encoding());
        let child_uri = crate::uri::path_to_file_uri(&child_canonical_id)?;

        Some(ResolvedComponentDocument {
            uri: child_uri,
            analysis: child_analysis,
            line_index: child_line_index,
        })
    }

    pub(super) fn resolve_component_document_for_import_binding(
        &self,
        parent_uri: &Uri,
        parent_analysis: &verter_session::FileAnalysisSnapshot,
        import_source: &str,
        binding_name: &str,
    ) -> Option<ResolvedComponentDocument> {
        let parent_canonical_id = uri_to_canonical_id(parent_uri);
        let import = parent_analysis
            .imports
            .iter()
            .find(|import| import.source == import_source);
        let mut resolved_targets = Vec::new();
        if let Some(resolved) = import.and_then(|entry| entry.resolved_canonical_id.clone()) {
            resolved_targets.push(resolved);
        }
        if let Some(resolved) = self.resolve_import_specifier(&parent_canonical_id, import_source) {
            if !resolved_targets
                .iter()
                .any(|candidate| candidate == &resolved)
            {
                resolved_targets.push(resolved);
            }
        }

        let child_canonical_id = resolved_targets.into_iter().find_map(|resolved_target| {
            if is_default_export_component_carrier(&resolved_target) {
                return Some(resolved_target);
            }

            self.documents
                .host()
                .get_export_span_follow_reexports(&resolved_target, binding_name)
                .map(|(resolved_id, _, _)| resolved_id)
                .filter(|resolved_id| is_default_export_component_carrier(resolved_id))
        })?;

        let child_analysis = self.documents.host().get_analysis(&child_canonical_id)?;
        let child_source = self.documents.host().get_source(&child_canonical_id)?;
        let child_line_index = LineIndex::new(&child_source, self.documents.encoding());
        let child_uri = crate::uri::path_to_file_uri(&child_canonical_id)?;

        Some(ResolvedComponentDocument {
            uri: child_uri,
            analysis: child_analysis,
            line_index: child_line_index,
        })
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

        for element in &template.elements {
            if !element.is_component && element.tag != "template" {
                continue;
            }

            // For <template #slot> elements, find the parent component
            let (component, child) = if element.tag == "template" {
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
                let child = match self.resolve_component_document_for_usage(uri, &analysis, comp) {
                    Some(child) => child,
                    None => continue,
                };
                (comp, child)
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
                let child = match self.resolve_component_document_for_usage(uri, &analysis, comp) {
                    Some(c) => c,
                    None => continue,
                };
                (comp, child)
            };

            // ── Props ───────────────────────────────────────────────
            for prop in &component.props {
                if offset >= prop.name_span.start && offset < prop.name_span.end {
                    let mut locations = Vec::new();

                    // For shorthand props, also resolve the parent binding
                    if prop.is_shorthand {
                        if let Some(parent_def) = self.resolve_template_identifier(
                            uri,
                            &analysis,
                            &doc.line_index,
                            &prop.name,
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

                    // Find matching prop field in child's defineProps
                    let mut child_found = false;
                    for mac in child.analysis.macros.iter() {
                        if let Some(prop_field) =
                            mac.prop_fields.iter().find(|f| f.name == prop.name)
                        {
                            if let Some(loc) =
                                location_from_span(&child.uri, &child.line_index, prop_field.span)
                            {
                                locations.push(loc);
                                child_found = true;
                            }
                        }
                    }
                    // Fallback: template-level prop definitions
                    if !child_found {
                        if let Some(child_template) = child.analysis.template.as_ref() {
                            if let Some(prop_def) = child_template
                                .prop_definitions
                                .iter()
                                .find(|d| d.name == prop.name)
                            {
                                if let Some(loc) =
                                    location_from_span(&child.uri, &child.line_index, prop_def.span)
                                {
                                    locations.push(loc);
                                    child_found = true;
                                }
                            }
                        }
                    }
                    // Final fallback: navigate to child file
                    if !child_found && !prop.is_shorthand {
                        locations.push(Location {
                            uri: child.uri.clone(),
                            range: Range::default(),
                        });
                    }

                    if !locations.is_empty() {
                        return Some(goto_response_from_locations(locations));
                    }
                }
            }

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

    /// Resolve a rename position on a `<Child prop=…>` usage to the child's prop
    /// DECLARATION identity: the child's `{carrier}.ts` PUBLIC-API provider path,
    /// the prop name, and the prop's `.vue` declaration span.
    ///
    /// This drives the provider-AGNOSTIC synthesis of the cross-file rename's
    /// child-declaration leg (a provider's own `textDocument/rename` may not
    /// enumerate that leg across the synthesized API surface — tgo does not). It
    /// reuses the SAME Vue resolution the goto-definition props branch
    /// ([`Self::try_component_contract_definition`]) uses: walk the template
    /// components, find the usage whose prop NAME span contains the cursor, resolve
    /// the child component, and read the matching `defineProps` field's `.vue` decl
    /// span from the child's analysis macros.
    ///
    /// Returns `None` (the caller then synthesizes nothing — never a usage-only
    /// partial) when the cursor is not on a child component's prop name, the child
    /// is not a resolvable component, or the prop is not declared in the child's
    /// macros (a template-only / unresolved prop has no inline API-surface span to
    /// rename).
    pub(super) fn resolve_child_prop_rename_target(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> Option<ChildPropRenameTarget> {
        let doc = self.documents.get(uri)?;
        let analysis = self.documents.get_analysis(uri)?;
        let template = analysis.template.as_ref()?;
        let offset = doc.line_index.position_to_offset(position)?;

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

            let child = self.resolve_component_document_for_usage(uri, &analysis, component)?;

            // The prop's `.vue` declaration span comes from the child's
            // `defineProps` macro fields — the typed identity the API generator
            // keys its source-map token on. A prop only declared via the template
            // (no macro field) has no inline API-surface member, so it is NOT a
            // synthesizable declaration-rename target here.
            let prop_decl_span = child.analysis.macros.iter().find_map(|mac| {
                mac.prop_fields
                    .iter()
                    .find(|field| field.name == prop.name)
                    .map(|field| field.span)
            })?;

            let child_canonical = uri_to_canonical_id(&child.uri);
            let child_carrier_api_path =
                verter_workspace::carrier_api_provider_path(&child_canonical);

            return Some(ChildPropRenameTarget {
                child_carrier_api_path,
                prop_name: prop.name.clone(),
                prop_decl_span,
            });
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

    /// Post-process type provider definition results to follow barrel re-exports.
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
    pub(super) fn resolve_slot_name_definition(
        &self,
        child: &ResolvedComponentDocument,
        slot_name: &str,
    ) -> Option<GotoDefinitionResponse> {
        // Check defineSlots macro first
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineSlots {
                continue;
            }
            if let Some(slot_field) = mac.slot_fields.iter().find(|f| f.name == slot_name) {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, slot_field.span)
                {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }

        // Fallback: template-level DefinedSlot
        if let Some(child_template) = child.analysis.template.as_ref() {
            if let Some(defined_slot) = child_template
                .defined_slots
                .iter()
                .find(|s| s.name == slot_name)
            {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, defined_slot.span)
                {
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
        // Check defineSlots macro
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineSlots {
                continue;
            }
            if let Some(slot_field) = mac.slot_fields.iter().find(|f| f.name == slot_name) {
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
        }

        None
    }

    /// Resolve a child component's analysis from an import source path.
    ///
    /// Tries three strategies:
    /// 1. Relative imports → resolve against the parent's directory
    /// 2. Path alias resolution via tsconfig.json
    /// 3. Direct lookup (bare specifiers)
    pub(super) fn resolve_component(
        &self,
        parent_uri: &Uri,
        import_source: &str,
    ) -> Option<verter_session::FileAnalysisSnapshot> {
        let canonical_id = uri_to_canonical_id(parent_uri);
        resolve_component_for(self.documents.host(), &canonical_id, import_source)
    }

    /// Resolve a child component with full context for cross-file editing.
    ///
    /// When `component_name` is provided and the import resolves to a non-`.vue`
    /// file (e.g. a barrel `index.ts`), follows re-export chains via
    /// `get_export_span_follow_reexports` to reach the terminal `.vue` file.
    pub(super) fn resolve_component_context(
        &self,
        parent_uri: &Uri,
        import_source: &str,
        component_name: Option<&str>,
    ) -> Option<crate::features::cross_file::ChildComponentContext> {
        let canonical_id = uri_to_canonical_id(parent_uri);

        // Resolve the child's canonical ID
        let mut child_canonical_id = self
            .resolve_import_specifier(&canonical_id, import_source)
            .unwrap_or_else(|| {
                if import_source.starts_with('.') {
                    let parts: Vec<&str> = canonical_id.split('/').collect();
                    let dir = parts[..parts.len().saturating_sub(1)].join("/");
                    resolve_import_path(&dir, import_source)
                } else {
                    import_source.to_string()
                }
            });

        // Follow barrel re-export chains: if the resolved file is not a .vue file
        // and we know the component name, look up the re-export chain to find the
        // terminal .vue file (e.g. ./components/index.ts → ./components/Button.vue).
        if !is_default_export_component_carrier(&child_canonical_id) {
            if let Some(name) = component_name {
                // Ensure the barrel file is loaded so we can inspect its exports
                if self
                    .documents
                    .host()
                    .get_analysis(&child_canonical_id)
                    .is_none()
                {
                    self.documents.host().ensure_loaded(&child_canonical_id);
                }
                if let Some((resolved_id, _, _)) = self
                    .documents
                    .host()
                    .get_export_span_follow_reexports(&child_canonical_id, name)
                {
                    if is_default_export_component_carrier(&resolved_id) {
                        child_canonical_id = resolved_id;
                    }
                }
            }
        }

        if self
            .documents
            .host()
            .get_source(&child_canonical_id)
            .is_none()
            || self
                .documents
                .host()
                .get_analysis(&child_canonical_id)
                .is_none()
        {
            if !self.documents.host().ensure_loaded(&child_canonical_id) {
                return None;
            }
            // ANALYSIS-facing (NOT IDE-sync): this drives the shared compile so
            // the imported component's analysis is populated, then reads
            // `get_analysis` below — it does NOT consume IDE TSX. The result is
            // ignored; the compile side-effect lands the analysis before any
            // (Main-less-carrier) `MissingVirtualNode`, so a Svelte child's
            // analysis still resolves. Kept on `ensure_compiled` deliberately.
            let profile = self.documents.tsx_profile.read().clone();
            let _ = self
                .documents
                .host
                .ensure_compiled(&child_canonical_id, &profile);
        }

        let analysis = self
            .resolve_component(parent_uri, import_source)
            .or_else(|| self.documents.host().get_analysis(&child_canonical_id))?;

        // If the analysis came from the barrel file but we resolved to a .vue file,
        // prefer the .vue file's analysis for accurate prop/emit information.
        let analysis = if is_default_export_component_carrier(&child_canonical_id) {
            self.documents
                .host()
                .get_analysis(&child_canonical_id)
                .unwrap_or(analysis)
        } else {
            analysis
        };

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
                let public_api = self
                    .documents
                    .host()
                    .get_public_api(&child.canonical_id)
                    .map(|api| api.code.to_string());
                Some(hover::build_child_component_hover(
                    &target.component_name,
                    &target.import_source,
                    &child.analysis,
                    public_api.as_deref(),
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
                let public_api = self
                    .documents
                    .host()
                    .get_public_api(&crate::documents::uri_to_canonical_id(&child.uri))
                    .map(|api| api.code.to_string());
                Some(hover::build_child_component_hover(
                    &target.binding_name,
                    &target.import_source,
                    &child.analysis,
                    public_api.as_deref(),
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
        // Pre-fix the hand-rolled body omitted the `//?/` extended-prefix strip
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
