use rustc_hash::FxHashSet;
use verter_semantic::analysis::type_eval::DeclarationId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExportTarget {
    pub source_canonical_id: Option<String>,
    pub source_name: String,
}

pub trait DeclarationMetadataResolver {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<ResolvedExportTarget>;

    fn get_export_span_follow_reexports(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_span::Span>;

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId>;

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String>;

    fn resolve_direct_type_reexport_target(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<(String, String)> {
        None
    }

    fn resolve_local_import_symbol_target(
        &self,
        _dep_canonical: &str,
        _resolved_name: &str,
    ) -> Option<(String, String)> {
        None
    }

    fn resolve_local_export_symbol_target(
        &self,
        _canonical_source: &str,
        _exported_name: &str,
    ) -> Option<String> {
        None
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        _canonical_source: &str,
        _resolved_name: &str,
    ) -> Option<ResolvedLocalTypeSymbolMetadata> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedDeclarationKind {
    Interface,
    TypeAlias,
    Class,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedLocalTypeSymbolMetadata {
    pub kind: ResolvedDeclarationKind,
    pub span: verter_span::Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTypeDeclaration {
    pub requested_name: String,
    pub declaration_id: Option<DeclarationId>,
    pub resolved_name: String,
    pub canonical_source: String,
    pub span: verter_span::Span,
    pub kind: ResolvedDeclarationKind,
    pub text: Option<String>,
}

fn resolve_local_symbol_details<R: DeclarationMetadataResolver>(
    resolver: &R,
    canonical_source: &str,
    resolved_name: &str,
    fallback_span: verter_span::Span,
    source: Option<&str>,
) -> (ResolvedDeclarationKind, verter_span::Span, Option<String>) {
    if let Some(symbol) =
        resolver.resolve_local_type_symbol_metadata(canonical_source, resolved_name)
    {
        if let Some(source) = source {
            let (_, derived_span, derived_text) =
                extract_declaration_details(source, symbol.span, resolved_name);
            if derived_text.is_some() {
                let _ = derived_span;
                return (symbol.kind, symbol.span, derived_text);
            }
            return (
                symbol.kind,
                symbol.span,
                slice_declaration_text(source, symbol.span),
            );
        }
        return (symbol.kind, symbol.span, None);
    }

    source
        .map(|source| extract_declaration_details(source, fallback_span, resolved_name))
        .unwrap_or((ResolvedDeclarationKind::Unknown, fallback_span, None))
}

pub fn resolve_type_declaration<R: DeclarationMetadataResolver>(
    resolver: &R,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    let (canonical_source, resolved_name) =
        if let Some(export) = resolver.resolve_export_target(dep_canonical, requested_name) {
            (
                export
                    .source_canonical_id
                    .unwrap_or_else(|| dep_canonical.to_string()),
                export.source_name,
            )
        } else if let Some((followed_canonical, followed_name)) =
            resolver.resolve_direct_type_reexport_target(dep_canonical, requested_name)
        {
            (followed_canonical, followed_name)
        } else if let Some((followed_canonical, followed_name)) =
            resolver.resolve_local_import_symbol_target(dep_canonical, requested_name)
        {
            (followed_canonical, followed_name)
        } else {
            follow_direct_type_reexport_chain(resolver, dep_canonical, requested_name)
                .unwrap_or_else(|| (dep_canonical.to_string(), requested_name.to_string()))
        };

    if canonical_source == dep_canonical {
        if let Some((followed_canonical, followed_name)) =
            follow_local_import_symbol_target(resolver, dep_canonical, resolved_name.as_str())
        {
            if followed_canonical != canonical_source || followed_name != resolved_name {
                let followed = resolve_type_declaration(
                    resolver,
                    followed_canonical.as_str(),
                    followed_name.as_str(),
                );
                if followed.canonical_source != canonical_source
                    || followed.resolved_name != resolved_name
                    || followed.kind != ResolvedDeclarationKind::Unknown
                    || followed.text.is_some()
                {
                    return ResolvedTypeDeclaration {
                        requested_name: requested_name.to_string(),
                        declaration_id: followed.declaration_id,
                        resolved_name: followed.resolved_name,
                        canonical_source: followed.canonical_source,
                        span: followed.span,
                        kind: followed.kind,
                        text: followed.text,
                    };
                }
            }
        }
    }

    let export_span = resolver
        .get_export_span_follow_reexports(dep_canonical, requested_name)
        .unwrap_or_default();
    // source-text reparse path retired. The graph
    // surface (`resolve_local_type_symbol_metadata`) is the
    // authoritative kind/span carrier; declaration text is no longer
    // populated by the resolver.
    let (kind, span, text) = resolve_local_symbol_details(
        resolver,
        canonical_source.as_str(),
        resolved_name.as_str(),
        export_span,
        None,
    );
    let declaration_id =
        resolver.type_declaration_id(canonical_source.as_str(), resolved_name.as_str());

    if kind == ResolvedDeclarationKind::Unknown {
        // The same-file local_export rerouting (e.g.
        // `export { Foo as Lt }`) stays — it consumes only graph
        // metadata via `resolve_local_export_symbol_target` and
        // `resolve_local_type_symbol_metadata`.
        if let Some(local_name) = resolver
            .resolve_local_export_symbol_target(canonical_source.as_str(), resolved_name.as_str())
        {
            let followed_details = resolve_local_symbol_details(
                resolver,
                canonical_source.as_str(),
                local_name.as_str(),
                export_span,
                None,
            );
            if followed_details.0 != ResolvedDeclarationKind::Unknown
                || followed_details.2.is_some()
            {
                return ResolvedTypeDeclaration {
                    requested_name: requested_name.to_string(),
                    declaration_id: resolver
                        .type_declaration_id(canonical_source.as_str(), local_name.as_str()),
                    resolved_name: local_name,
                    canonical_source,
                    span: followed_details.1,
                    kind: followed_details.0,
                    text: followed_details.2,
                };
            }
        }
    }

    if kind == ResolvedDeclarationKind::Unknown
        && text.is_none()
        && canonical_source == dep_canonical
    {
        if let Some((followed_canonical, followed_name)) =
            follow_local_import_symbol_target(resolver, dep_canonical, resolved_name.as_str())
        {
            if followed_canonical != canonical_source || followed_name != resolved_name {
                let followed = resolve_type_declaration(
                    resolver,
                    followed_canonical.as_str(),
                    followed_name.as_str(),
                );
                if followed.canonical_source != canonical_source
                    || followed.resolved_name != resolved_name
                    || followed.kind != ResolvedDeclarationKind::Unknown
                    || followed.text.is_some()
                {
                    return ResolvedTypeDeclaration {
                        requested_name: requested_name.to_string(),
                        declaration_id: followed.declaration_id,
                        resolved_name: followed.resolved_name,
                        canonical_source: followed.canonical_source,
                        span: followed.span,
                        kind: followed.kind,
                        text: followed.text,
                    };
                }
            }
        }

        if let Some((followed_canonical, followed_name)) =
            follow_direct_type_reexport_chain(resolver, dep_canonical, requested_name)
        {
            if followed_canonical != canonical_source || followed_name != resolved_name {
                // source-text reparse path retired.
                // The leaf graph metadata
                // (`resolve_local_type_symbol_metadata`) is the
                // authoritative kind/span carrier when the chain
                // moves; the leaf source is no longer reread.
                let followed_details = resolve_local_symbol_details(
                    resolver,
                    followed_canonical.as_str(),
                    followed_name.as_str(),
                    export_span,
                    None,
                );
                if followed_details.0 != ResolvedDeclarationKind::Unknown
                    || followed_details.2.is_some()
                {
                    return ResolvedTypeDeclaration {
                        requested_name: requested_name.to_string(),
                        declaration_id: resolver.type_declaration_id(
                            followed_canonical.as_str(),
                            followed_name.as_str(),
                        ),
                        resolved_name: followed_name,
                        canonical_source: followed_canonical,
                        span: followed_details.1,
                        kind: followed_details.0,
                        text: followed_details.2,
                    };
                }
            }
        }
    }

    ResolvedTypeDeclaration {
        requested_name: requested_name.to_string(),
        declaration_id,
        resolved_name,
        canonical_source,
        span,
        kind,
        text,
    }
}

pub fn resolve_local_type_declaration<R: DeclarationMetadataResolver>(
    resolver: &R,
    canonical_source: &str,
    resolved_name: &str,
    span: verter_span::Span,
) -> ResolvedTypeDeclaration {
    // source-text reparse path retired. The graph
    // metadata (`resolve_local_type_symbol_metadata`) is the
    // authoritative kind/span carrier. When metadata is available
    // its `kind` and `span` are preferred; otherwise the caller's
    // `span` is preserved and `kind` is `Unknown`. Declaration text
    // is no longer populated by the resolver.
    let (kind, resolved_span) = resolver
        .resolve_local_type_symbol_metadata(canonical_source, resolved_name)
        .map(|metadata| (metadata.kind, metadata.span))
        .unwrap_or((ResolvedDeclarationKind::Unknown, span));
    let declaration_id = resolver.type_declaration_id(canonical_source, resolved_name);

    ResolvedTypeDeclaration {
        requested_name: resolved_name.to_string(),
        declaration_id,
        resolved_name: resolved_name.to_string(),
        canonical_source: canonical_source.to_string(),
        span: resolved_span,
        kind,
        text: None,
    }
}

pub fn resolve_direct_local_type_declaration<R: DeclarationMetadataResolver>(
    resolver: &R,
    canonical_source: &str,
    resolved_name: &str,
) -> Option<ResolvedTypeDeclaration> {
    let metadata = resolver.resolve_local_type_symbol_metadata(canonical_source, resolved_name)?;
    Some(resolve_local_type_declaration(
        resolver,
        canonical_source,
        resolved_name,
        metadata.span,
    ))
}

fn follow_direct_type_reexport_chain<R: DeclarationMetadataResolver>(
    resolver: &R,
    dep_canonical: &str,
    requested_name: &str,
) -> Option<(String, String)> {
    let mut current_canonical = dep_canonical.to_string();
    let mut current_name = requested_name.to_string();
    let mut visited = FxHashSet::default();

    loop {
        if !visited.insert((current_canonical.clone(), current_name.clone())) {
            return Some((current_canonical, current_name));
        }

        if let Some(next) = resolver
            .resolve_direct_type_reexport_target(current_canonical.as_str(), current_name.as_str())
        {
            current_canonical = next.0;
            current_name = next.1;
            continue;
        }

        return Some((current_canonical, current_name));
    }
}

fn follow_local_import_symbol_target<R: DeclarationMetadataResolver>(
    resolver: &R,
    dep_canonical: &str,
    resolved_name: &str,
) -> Option<(String, String)> {
    resolver.resolve_local_import_symbol_target(dep_canonical, resolved_name)
}

fn extract_declaration_details(
    source: &str,
    span: verter_span::Span,
    resolved_name: &str,
) -> (ResolvedDeclarationKind, verter_span::Span, Option<String>) {
    if let Some((kind, start)) = find_named_declaration_start(source, span, resolved_name) {
        if let Some((declaration_span, text)) = extract_named_declaration_text(source, start, kind)
        {
            return (kind, declaration_span, Some(text));
        }
    }

    if span.end > span.start {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < source.len() && end <= source.len() {
            return (
                ResolvedDeclarationKind::Unknown,
                span,
                source.get(start..end).map(|text| text.trim().to_string()),
            );
        }
    }

    (ResolvedDeclarationKind::Unknown, span, None)
}

fn slice_declaration_text(source: &str, span: verter_span::Span) -> Option<String> {
    let start = span.start as usize;
    let end = span.end as usize;
    (start < end && end <= source.len()).then(|| source[start..end].trim().to_string())
}

fn find_named_declaration_start(
    source: &str,
    span: verter_span::Span,
    resolved_name: &str,
) -> Option<(ResolvedDeclarationKind, usize)> {
    let search_end = if span.start == 0 && span.end == 0 {
        source.len()
    } else {
        (span.end as usize).min(source.len())
    };
    let haystack = source.get(..search_end).unwrap_or(source);
    let patterns = [
        (
            ResolvedDeclarationKind::Interface,
            format!("interface {resolved_name}"),
        ),
        (
            ResolvedDeclarationKind::TypeAlias,
            format!("type {resolved_name}"),
        ),
        (
            ResolvedDeclarationKind::Class,
            format!("class {resolved_name}"),
        ),
    ];

    patterns
        .into_iter()
        .filter_map(|(kind, needle)| {
            haystack.rfind(&needle).and_then(|start| {
                let after = start + needle.len();
                if after < haystack.len() {
                    let next = haystack.as_bytes()[after];
                    if next.is_ascii_alphanumeric() || next == b'_' {
                        return None;
                    }
                }
                Some((kind, start))
            })
        })
        .max_by_key(|(_, start)| *start)
}

fn extract_named_declaration_text(
    source: &str,
    keyword_start: usize,
    kind: ResolvedDeclarationKind,
) -> Option<(verter_span::Span, String)> {
    let line_start = source[..keyword_start]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);

    let end = match kind {
        ResolvedDeclarationKind::Interface | ResolvedDeclarationKind::Class => {
            let brace_start = source.get(keyword_start..)?.find('{')? + keyword_start;
            find_matching_brace(source, brace_start).map(|idx| idx + 1)
        }
        ResolvedDeclarationKind::TypeAlias => find_top_level_semicolon(source, keyword_start)
            .map(|idx| idx + 1)
            .or_else(|| {
                source[keyword_start..]
                    .find('\n')
                    .map(|idx| keyword_start + idx)
            }),
        ResolvedDeclarationKind::Unknown => None,
    }?;

    source.get(line_start..end).map(|text| {
        (
            verter_span::Span::new(line_start as u32, end as u32),
            text.trim().to_string(),
        )
    })
}

fn find_matching_brace(source: &str, brace_start: usize) -> Option<usize> {
    let bytes = source.get(brace_start..)?.as_bytes();
    let mut depth = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b'\'' | b'"' | b'`' => {
                i += 1;
                while i < bytes.len() && bytes[i] != ch {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(brace_start + i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_top_level_semicolon(source: &str, start: usize) -> Option<usize> {
    let bytes = source.get(start..)?.as_bytes();
    let mut depth = 0u32;
    for (i, &ch) in bytes.iter().enumerate() {
        match ch {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return Some(start + i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;

    #[derive(Default)]
    struct FakeResolver {
        exports: FxHashMap<(String, String), ResolvedExportTarget>,
        spans: FxHashMap<(String, String), verter_span::Span>,
        ids: FxHashMap<(String, String), DeclarationId>,
        dep_canonicals: FxHashMap<(String, String), String>,
        direct_reexports: FxHashMap<(String, String), (String, String)>,
        local_import_symbol_targets: FxHashMap<(String, String), (String, String)>,
        local_export_symbol_targets: FxHashMap<(String, String), String>,
        local_type_symbol_metadata: FxHashMap<(String, String), ResolvedLocalTypeSymbolMetadata>,
    }

    impl DeclarationMetadataResolver for FakeResolver {
        fn resolve_export_target(
            &self,
            dep_canonical: &str,
            requested_name: &str,
        ) -> Option<ResolvedExportTarget> {
            self.exports
                .get(&(dep_canonical.to_string(), requested_name.to_string()))
                .cloned()
        }

        fn get_export_span_follow_reexports(
            &self,
            dep_canonical: &str,
            requested_name: &str,
        ) -> Option<verter_span::Span> {
            self.spans
                .get(&(dep_canonical.to_string(), requested_name.to_string()))
                .copied()
        }

        fn type_declaration_id(
            &self,
            canonical_source: &str,
            resolved_name: &str,
        ) -> Option<DeclarationId> {
            self.ids
                .get(&(canonical_source.to_string(), resolved_name.to_string()))
                .copied()
        }

        fn resolve_type_dependency_canonical(
            &self,
            from_canonical: &str,
            import_source: &str,
        ) -> Option<String> {
            self.dep_canonicals
                .get(&(from_canonical.to_string(), import_source.to_string()))
                .cloned()
        }

        fn resolve_direct_type_reexport_target(
            &self,
            dep_canonical: &str,
            requested_name: &str,
        ) -> Option<(String, String)> {
            self.direct_reexports
                .get(&(dep_canonical.to_string(), requested_name.to_string()))
                .cloned()
        }

        fn resolve_local_import_symbol_target(
            &self,
            dep_canonical: &str,
            resolved_name: &str,
        ) -> Option<(String, String)> {
            self.local_import_symbol_targets
                .get(&(dep_canonical.to_string(), resolved_name.to_string()))
                .cloned()
        }

        fn resolve_local_export_symbol_target(
            &self,
            canonical_source: &str,
            exported_name: &str,
        ) -> Option<String> {
            self.local_export_symbol_targets
                .get(&(canonical_source.to_string(), exported_name.to_string()))
                .cloned()
        }

        fn resolve_local_type_symbol_metadata(
            &self,
            canonical_source: &str,
            resolved_name: &str,
        ) -> Option<ResolvedLocalTypeSymbolMetadata> {
            self.local_type_symbol_metadata
                .get(&(canonical_source.to_string(), resolved_name.to_string()))
                .copied()
        }
    }

    #[test]
    fn declaration_text_does_not_match_substring_names() {
        let source = r#"
interface PropsExtended { a: string }
interface Props { b: number }
"#;
        let span = verter_span::Span::new(0, source.len() as u32);

        let (_, _, text) = extract_declaration_details(source, span, "Props");

        assert_eq!(text.as_deref(), Some("interface Props { b: number }"));
    }

    #[test]
    fn declaration_text_handles_braces_inside_string_literals() {
        let source = r#"
type Props = {
  label: "{not a brace}";
  nested: { ok: true };
};
"#;
        let span = verter_span::Span::new(0, source.len() as u32);

        let (_, _, text) = extract_declaration_details(source, span, "Props");

        assert!(text
            .as_deref()
            .is_some_and(|text| text.contains("\"{not a brace}\"")));
        assert!(text
            .as_deref()
            .is_some_and(|text| text.contains("nested: { ok: true }")));
    }

    #[test]
    fn resolve_type_declaration_follows_direct_reexport_chain_when_entry_lacks_decl() {
        let mut resolver = FakeResolver::default();
        resolver.spans.insert(
            ("/types.ts".to_string(), "Props".to_string()),
            verter_span::Span::new(0, 100),
        );
        resolver.direct_reexports.insert(
            ("/types.ts".to_string(), "Props".to_string()),
            ("/inner.ts".to_string(), "Props".to_string()),
        );
        resolver.local_type_symbol_metadata.insert(
            ("/inner.ts".to_string(), "Props".to_string()),
            ResolvedLocalTypeSymbolMetadata {
                kind: ResolvedDeclarationKind::Interface,
                span: verter_span::Span::new(0, 39),
            },
        );
        resolver
            .ids
            .insert(("/inner.ts".to_string(), "Props".to_string()), 7);

        let resolved = resolve_type_declaration(&resolver, "/types.ts", "Props");

        assert_eq!(resolved.canonical_source, "/inner.ts");
        assert_eq!(resolved.resolved_name, "Props");
        assert_eq!(resolved.declaration_id, Some(7));
        assert_eq!(resolved.kind, ResolvedDeclarationKind::Interface);
    }

    #[test]
    fn resolve_type_declaration_follows_same_file_local_export_symbol_when_span_points_to_alias() {
        let source = "type RouteLocationRaw = string;\nexport { RouteLocationRaw as Lt };";
        let alias_start = source.find("Lt").expect("alias should exist") as u32;
        let alias_end = alias_start + 2;
        let mut resolver = FakeResolver::default();
        resolver.spans.insert(
            ("/inner.ts".to_string(), "Lt".to_string()),
            verter_span::Span::new(alias_start, alias_end),
        );
        resolver.local_export_symbol_targets.insert(
            ("/inner.ts".to_string(), "Lt".to_string()),
            "RouteLocationRaw".to_string(),
        );
        resolver.local_type_symbol_metadata.insert(
            ("/inner.ts".to_string(), "RouteLocationRaw".to_string()),
            ResolvedLocalTypeSymbolMetadata {
                kind: ResolvedDeclarationKind::TypeAlias,
                span: verter_span::Span::new(0, 31),
            },
        );
        resolver.ids.insert(
            ("/inner.ts".to_string(), "RouteLocationRaw".to_string()),
            11,
        );

        let resolved = resolve_type_declaration(&resolver, "/inner.ts", "Lt");

        assert_eq!(resolved.canonical_source, "/inner.ts");
        assert_eq!(resolved.resolved_name, "RouteLocationRaw");
        assert_eq!(resolved.declaration_id, Some(11));
        assert_eq!(resolved.kind, ResolvedDeclarationKind::TypeAlias);
        // source-text reparse retired. The
        // local_export symbol target rerouting still resolves the
        // declaration via graph metadata; only the `text` field is
        // no longer populated.
        assert_eq!(resolved.text, None);
    }

    // `resolve_type_declaration_uses_cached_*_without_reparsing_source`
    // were negative tests asserting that cached reexport / local-import
    // routes did NOT trigger source reparsing for the original (barrel /
    // owner) canonical id. Under the graph-only resolver, the
    // resolver no longer reads source at all (the trait method is
    // deleted), so these "did not reread" assertions are now
    // trivially satisfied. The tests are obsolete and removed.

    #[test]
    fn resolve_type_declaration_prefers_cached_local_symbol_metadata_for_text() {
        let mut resolver = FakeResolver::default();
        let source = "type Props = {\n  label: string\n};\n";
        resolver.local_type_symbol_metadata.insert(
            ("/types.ts".to_string(), "Props".to_string()),
            ResolvedLocalTypeSymbolMetadata {
                kind: ResolvedDeclarationKind::TypeAlias,
                span: verter_span::Span::new(0, source.len() as u32),
            },
        );

        let resolved = resolve_type_declaration(&resolver, "/types.ts", "Props");

        assert_eq!(resolved.kind, ResolvedDeclarationKind::TypeAlias);
        assert_eq!(
            resolved.span,
            verter_span::Span::new(0, source.len() as u32)
        );
        // graph metadata is the kind/span carrier;
        // declaration text is no longer populated.
        assert_eq!(resolved.text, None);
    }

    // ----------------------------------------------------------------------
    // graph-only decoupling tests for the three
    // production source-reading callsites (former lines 184, 261, 308).
    // Each test seeds graph metadata and asserts the resolver returns
    // graph kind/span/declaration_id while `declaration.text` is `None`
    // — the architectural contract that resolver-core consumes only
    // graph data, not raw source text.
    //
    // : the source-reparse path populated `text` from the
    // sources map → these tests would have FAILED with `Some(_)`.
    // : the production path no longer threads source
    // text through → tests PASS with `text == None`.

    #[test]
    fn declaration_metadata_resolves_local_symbol_via_graph_only() {
        // Discrimination test for the local-symbol resolution path.
        // The graph metadata (`local_type_symbol_metadata`) is seeded;
        // the resolver returns kind/span/declaration_id from the graph
        // and leaves `text` None — there is no source-reading
        // fallback. A regression that re-introduces a source-reparse
        // path would populate `text` to `Some(...)` and fail this
        // assertion.
        let mut resolver = FakeResolver::default();
        let source_len = "type Props = { label: string };\n".len() as u32;
        resolver.local_type_symbol_metadata.insert(
            ("/types.ts".to_string(), "Props".to_string()),
            ResolvedLocalTypeSymbolMetadata {
                kind: ResolvedDeclarationKind::TypeAlias,
                span: verter_span::Span::new(0, source_len),
            },
        );
        resolver
            .ids
            .insert(("/types.ts".to_string(), "Props".to_string()), 42);

        let resolved = resolve_type_declaration(&resolver, "/types.ts", "Props");

        // Graph fields populated correctly.
        assert_eq!(resolved.canonical_source, "/types.ts");
        assert_eq!(resolved.resolved_name, "Props");
        assert_eq!(resolved.kind, ResolvedDeclarationKind::TypeAlias);
        assert_eq!(resolved.span, verter_span::Span::new(0, source_len));
        assert_eq!(resolved.declaration_id, Some(42));

        // Negative assertion: text MUST be None — proves the source-
        // reparse path is gone.
        assert_eq!(
            resolved.text, None,
            "resolve_type_declaration must NOT thread source \
             text into ResolvedTypeDeclaration.text (graph-only resolver)"
        );
    }

    #[test]
    fn declaration_metadata_follows_reexport_chain_via_graph_only() {
        // discrimination test for the reexport-
        // following path (former callsite at
        // declaration_metadata.rs:261). A barrel (`/types.ts`) re-
        // exports `Props` from `/inner.ts` via `direct_reexports`;
        // graph metadata at the leaf (`/inner.ts`) provides
        // kind/span. The resolver follows the chain and returns the
        // leaf's graph fields with `text == None`. Pre-deletion,
        // the leaf source-reparse arm populated `text` to
        // `Some(...)` and failed the negative assertion.
        let mut resolver = FakeResolver::default();
        resolver.direct_reexports.insert(
            ("/types.ts".to_string(), "Props".to_string()),
            ("/inner.ts".to_string(), "Props".to_string()),
        );
        let leaf_source_len = "interface Props { label: string }\n".len() as u32;
        // Graph metadata at leaf — what the path uses.
        resolver.local_type_symbol_metadata.insert(
            ("/inner.ts".to_string(), "Props".to_string()),
            ResolvedLocalTypeSymbolMetadata {
                kind: ResolvedDeclarationKind::Interface,
                span: verter_span::Span::new(0, leaf_source_len),
            },
        );
        resolver
            .ids
            .insert(("/inner.ts".to_string(), "Props".to_string()), 7);

        let resolved = resolve_type_declaration(&resolver, "/types.ts", "Props");

        // Graph fields point at the leaf (chain followed).
        assert_eq!(resolved.canonical_source, "/inner.ts");
        assert_eq!(resolved.resolved_name, "Props");
        assert_eq!(resolved.kind, ResolvedDeclarationKind::Interface);
        assert_eq!(resolved.span, verter_span::Span::new(0, leaf_source_len));
        assert_eq!(resolved.declaration_id, Some(7));

        // Negative assertion: text MUST be None — proves the leaf
        // source-reparse arm at line 261 no longer runs.
        assert_eq!(
            resolved.text, None,
            "reexport-chain following must NOT re-read leaf \
             source to populate ResolvedTypeDeclaration.text"
        );
    }

    #[test]
    fn declaration_metadata_extracts_details_via_graph_only() {
        // discrimination test for
        // `resolve_local_type_declaration` (former callsite at
        // declaration_metadata.rs:308). The graph metadata
        // (`local_type_symbol_metadata`) provides kind/span; the
        // resolver returns those plus `text == None`. Pre-deletion,
        // the source-reparse path populated `text` to `Some(...)`
        // and failed the negative assertion.
        let mut resolver = FakeResolver::default();
        let source_len = "interface Props { label: string }".len() as u32;
        let graph_span = verter_span::Span::new(0, source_len);
        resolver.local_type_symbol_metadata.insert(
            ("/types.ts".to_string(), "Props".to_string()),
            ResolvedLocalTypeSymbolMetadata {
                kind: ResolvedDeclarationKind::Interface,
                span: graph_span,
            },
        );
        resolver
            .ids
            .insert(("/types.ts".to_string(), "Props".to_string()), 11);

        // Drive the same path resolve_local_type_declaration runs in.
        let resolved = resolve_local_type_declaration(&resolver, "/types.ts", "Props", graph_span);

        assert_eq!(resolved.canonical_source, "/types.ts");
        assert_eq!(resolved.resolved_name, "Props");
        assert_eq!(resolved.kind, ResolvedDeclarationKind::Interface);
        assert_eq!(resolved.span, graph_span);
        assert_eq!(resolved.declaration_id, Some(11));

        // Negative assertion: text MUST be None — proves
        // resolve_local_type_declaration no longer reads source text.
        assert_eq!(
            resolved.text, None,
            "resolve_local_type_declaration must NOT thread \
             source text into ResolvedTypeDeclaration.text"
        );
    }
}
