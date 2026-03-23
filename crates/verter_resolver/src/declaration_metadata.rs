use oxc_allocator::Allocator;
use rustc_hash::FxHashSet;
use verter_analysis::type_eval::DeclarationId;
use verter_core::utils::oxc::vue::resolve_type::extract_imported_type_bindings;

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

    fn read_source(&self, canonical_source: &str) -> Option<String>;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedDeclarationKind {
    Interface,
    TypeAlias,
    Class,
    Unknown,
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
        } else {
            follow_direct_type_reexport_chain(resolver, dep_canonical, requested_name)
                .unwrap_or_else(|| (dep_canonical.to_string(), requested_name.to_string()))
        };

    let export_span = resolver
        .get_export_span_follow_reexports(dep_canonical, requested_name)
        .unwrap_or_default();
    let (kind, span, text) = resolver
        .read_source(canonical_source.as_str())
        .map(|source| extract_declaration_details(&source, export_span, resolved_name.as_str()))
        .unwrap_or((ResolvedDeclarationKind::Unknown, export_span, None));
    let declaration_id =
        resolver.type_declaration_id(canonical_source.as_str(), resolved_name.as_str());

    if kind == ResolvedDeclarationKind::Unknown
        && text.is_none()
        && canonical_source == dep_canonical
    {
        if let Some((followed_canonical, followed_name)) =
            follow_direct_type_reexport_chain(resolver, dep_canonical, requested_name)
        {
            if followed_canonical != canonical_source || followed_name != resolved_name {
                if let Some(source) = resolver.read_source(followed_canonical.as_str()) {
                    let followed_details =
                        extract_declaration_details(&source, export_span, followed_name.as_str());
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
    let (kind, resolved_span, text) = resolver
        .read_source(canonical_source)
        .map(|source| extract_declaration_details(&source, span, resolved_name))
        .unwrap_or((ResolvedDeclarationKind::Unknown, span, None));
    let declaration_id = resolver.type_declaration_id(canonical_source, resolved_name);

    ResolvedTypeDeclaration {
        requested_name: resolved_name.to_string(),
        declaration_id,
        resolved_name: resolved_name.to_string(),
        canonical_source: canonical_source.to_string(),
        span: resolved_span,
        kind,
        text,
    }
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

        let source = resolver.read_source(current_canonical.as_str())?;
        let alloc = Allocator::new();
        let extracted = extract_imported_type_bindings(&source, &alloc);

        let Some(reexport) = extracted
            .reexport_bindings
            .iter()
            .find(|binding| binding.local_name == current_name)
        else {
            return Some((current_canonical, current_name));
        };

        let Some(next_canonical) = resolver.resolve_type_dependency_canonical(
            current_canonical.as_str(),
            reexport.source.as_str(),
        ) else {
            return Some((current_canonical, current_name));
        };

        current_canonical = next_canonical;
        current_name = reexport.imported_name.clone();
    }
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
        sources: FxHashMap<String, String>,
        ids: FxHashMap<(String, String), DeclarationId>,
        dep_canonicals: FxHashMap<(String, String), String>,
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

        fn read_source(&self, canonical_source: &str) -> Option<String> {
            self.sources.get(canonical_source).cloned()
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
        resolver.sources.insert(
            "/types.ts".to_string(),
            r#"import { Props } from "./inner"; export type { Props };"#.to_string(),
        );
        resolver.sources.insert(
            "/inner.ts".to_string(),
            "export interface Props { label: string }".to_string(),
        );
        resolver.dep_canonicals.insert(
            ("/types.ts".to_string(), "./inner".to_string()),
            "/inner.ts".to_string(),
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
}
