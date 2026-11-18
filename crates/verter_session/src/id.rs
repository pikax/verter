//! Canonical ID normalization, virtual ID rendering, and relative import resolution.

use std::borrow::Cow;

use crate::types::{
    FileMeta, ParsedRawId, RawIdRejection, RejectedRawId, RequestMarker, VirtualNodeKind,
};

/// Normalize a path or bundler request id to its canonical id: forward
/// slashes, a lower-case Windows drive letter, no Windows extended-length
/// prefix, and no query or virtual-file suffix. Returns a borrowed view when
/// no transformation is needed.
///
/// This is the same normalization the host applies when it registers a
/// source, so two inputs that canonicalize equal name the same file. Bulk
/// callers use it to deduplicate inputs before
/// [`crate::VerterHost::upsert_many_for_compile`], which requires unique
/// canonical identities.
pub fn canonicalize_id(input: &str) -> Cow<'_, str> {
    let trimmed = input.trim();

    // Strip the Windows extended-length path prefix (`//?/` or `\\?\`) early.
    // OS canonicalization produces these on Windows and they break
    // query-parameter stripping (`?` in `//?/` is not a query separator).
    // The UNC form is tested first, because it also starts with the plain
    // prefix: `//?/UNC/server/share` keeps its server and share as
    // `//server/share`.
    let (unc_root, trimmed) = if let Some(rest) = trimmed
        .strip_prefix("//?/UNC/")
        .or_else(|| trimmed.strip_prefix("\\\\?\\UNC\\"))
    {
        ("//", rest)
    } else if let Some(rest) = trimmed
        .strip_prefix("//?/")
        .or_else(|| trimmed.strip_prefix("\\\\?\\"))
    {
        ("", rest)
    } else {
        ("", trimmed)
    };

    // Check if the path has a Windows drive letter that needs lowercasing
    // (e.g., `C:/...` or `C:\...` → `c:/...`)
    let needs_drive_lower = {
        let b = trimmed.as_bytes();
        unc_root.is_empty()
            && b.len() >= 2
            && b[0].is_ascii_uppercase()
            && b[1] == b':'
            && (b.len() == 2 || b[2] == b'/' || b[2] == b'\\')
    };

    // Fast path: no transformations needed — avoid allocations
    if trimmed.len() == input.len()
        && !trimmed.contains('\\')
        && !trimmed.contains('?')
        && !trimmed.contains("._VERTER_.")
        && !needs_drive_lower
    {
        return Cow::Borrowed(trimmed);
    }
    let mut s = format!("{unc_root}{}", trimmed.replace('\\', "/"));
    if let Some((base, _)) = s.split_once('?') {
        s = base.to_string();
    }
    if let Some((base, _)) = s.split_once("._VERTER_.") {
        s = base.to_string();
    }
    // Normalize Windows drive letter to lowercase
    if needs_drive_lower {
        // Safety: drive letter is always ASCII, so byte-level mutation is safe
        unsafe {
            s.as_bytes_mut()[0] = s.as_bytes()[0].to_ascii_lowercase();
        }
    }
    Cow::Owned(s)
}

/// Parse a raw request id: an LSP-format `._VERTER_.` id, or a bundler id
/// whose query names a carrier sub-module.
///
/// A bundler id without a marker is a main request whatever else its query
/// carries. With a marker, `type` selects the block (`script`, `template`,
/// `style`, any other value names a custom block) and `index` defaults to 0.
/// Query tokens the grammar does not own are ignored.
pub fn parse_raw_id(raw: &str) -> Result<ParsedRawId, RejectedRawId> {
    let trimmed = raw.trim();
    let normalized_raw: Cow<'_, str> = if trimmed.contains('\\') {
        Cow::Owned(trimmed.replace('\\', "/"))
    } else {
        Cow::Borrowed(trimmed)
    };

    if let Some((canonical, suffix)) = normalized_raw.split_once("._VERTER_.") {
        return parse_lsp_id(trimmed, canonical, suffix);
    }

    let (raw_path, query) = split_request_query(trimmed);
    let rejected = |reason, marker| RejectedRawId {
        reason,
        raw_path: raw_path.to_owned(),
        marker,
    };
    let canonical = canonicalize_id(raw_path).into_owned();
    let query = query.map(RequestQuery::read).unwrap_or_default();
    let marker = match (query.vue, query.verter) {
        (0, 0) => None,
        (1, 0) => Some(RequestMarker::Vue),
        (0, 1) => Some(RequestMarker::Verter),
        (vue, verter) if vue > 0 && verter > 0 => {
            return Err(rejected(RawIdRejection::BothMarkers, None));
        }
        (_, 0) => {
            return Err(rejected(
                RawIdRejection::RepeatedOwnedKey,
                Some(RequestMarker::Vue),
            ));
        }
        _ => {
            return Err(rejected(
                RawIdRejection::RepeatedOwnedKey,
                Some(RequestMarker::Verter),
            ));
        }
    };
    let node_kind = match marker {
        None => VirtualNodeKind::Main,
        Some(_) => query
            .node_kind()
            .map_err(|reason| rejected(reason, marker))?,
    };
    let block_type = match node_kind {
        VirtualNodeKind::Custom { .. } => query.types.first().copied().flatten(),
        _ => None,
    };
    Ok(ParsedRawId {
        canonical_id: canonical,
        node_kind,
        was_lsp_like: false,
        marker,
        lang: query.lang.map(str::to_owned),
        scoped: query.scoped,
        raw_path: raw_path.to_owned(),
        block_type: block_type.map(str::to_owned),
    })
}

fn parse_lsp_id(
    trimmed: &str,
    canonical: &str,
    suffix: &str,
) -> Result<ParsedRawId, RejectedRawId> {
    let raw_path = trimmed
        .split_once("._VERTER_.")
        .map_or(trimmed, |(path, _)| path);
    let suffix_index = |rest: &str| {
        rest.split('.')
            .next()
            .and_then(|p| p.parse::<usize>().ok())
            .unwrap_or(0)
    };
    let suffix_name = |rest: &str| {
        rest.split_once('.')
            .map(|(_, name)| name.to_owned())
            .filter(|name| !name.is_empty())
    };
    let mut lang = None;
    let mut block_type = None;
    let node_kind = if suffix.starts_with("bundle.ts") {
        VirtualNodeKind::Main
    } else if suffix.starts_with("options.ts") {
        VirtualNodeKind::Script
    } else if suffix.starts_with("render.tsx") {
        VirtualNodeKind::Template
    } else if let Some(rest) = suffix.strip_prefix("style.") {
        lang = suffix_name(rest);
        VirtualNodeKind::Style {
            index: suffix_index(rest),
        }
    } else if let Some(rest) = suffix.strip_prefix("custom.") {
        block_type = suffix_name(rest);
        VirtualNodeKind::Custom {
            index: suffix_index(rest),
        }
    } else {
        return Err(RejectedRawId {
            reason: RawIdRejection::UnknownVirtualSuffix,
            raw_path: raw_path.to_owned(),
            marker: None,
        });
    };
    Ok(ParsedRawId {
        canonical_id: canonicalize_id(canonical).into_owned(),
        node_kind,
        was_lsp_like: true,
        marker: None,
        lang,
        scoped: false,
        raw_path: raw_path.to_owned(),
        block_type,
    })
}

/// Split a request at its first query `?`. The `?` of a Windows
/// extended-length prefix (`//?/`, `\\?\`) belongs to the path.
fn split_request_query(request: &str) -> (&str, Option<&str>) {
    let path_start = if request.starts_with("//?/") || request.starts_with("\\\\?\\") {
        4
    } else {
        0
    };
    match request[path_start..].find('?') {
        Some(at) => (
            &request[..path_start + at],
            Some(&request[path_start + at + 1..]),
        ),
        None => (request, None),
    }
}

/// The query tokens the id grammar owns, as read from one request.
#[derive(Default)]
struct RequestQuery<'q> {
    vue: usize,
    verter: usize,
    types: Vec<Option<&'q str>>,
    indexes: Vec<Option<&'q str>>,
    lang: Option<&'q str>,
    scoped: bool,
}

impl<'q> RequestQuery<'q> {
    fn read(query: &'q str) -> Self {
        let mut read = Self::default();
        for chunk in query.split('&').filter(|chunk| !chunk.is_empty()) {
            let (key, value) = match chunk.split_once('=') {
                Some((key, value)) => (key, Some(value)),
                None => (chunk, None),
            };
            if value.is_none() {
                if let Some(ext) = key.strip_prefix("lang.") {
                    if read.lang.is_none() && !ext.is_empty() {
                        read.lang = Some(ext);
                    }
                    continue;
                }
            }
            match key.to_ascii_lowercase().as_str() {
                "vue" => read.vue += 1,
                "verter" => read.verter += 1,
                "type" => read.types.push(value),
                "index" => read.indexes.push(value),
                "lang" => {
                    if read.lang.is_none() {
                        read.lang = value.filter(|ext| !ext.is_empty());
                    }
                }
                "scoped" => read.scoped = true,
                _ => {}
            }
        }
        read
    }

    /// The block a marked request names.
    fn node_kind(&self) -> Result<VirtualNodeKind, RawIdRejection> {
        if self.types.len() > 1 || self.indexes.len() > 1 {
            return Err(RawIdRejection::RepeatedOwnedKey);
        }
        let block_type = self
            .types
            .first()
            .copied()
            .flatten()
            .filter(|block_type| !block_type.is_empty())
            .ok_or(RawIdRejection::MarkerWithoutType)?
            .to_ascii_lowercase();
        let index = match self.indexes.first() {
            None => 0,
            Some(value) => value
                .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|digits| digits.parse::<usize>().ok())
                .ok_or(RawIdRejection::InvalidIndex)?,
        };
        Ok(match block_type.as_str() {
            "script" => VirtualNodeKind::Script,
            "template" => VirtualNodeKind::Template,
            "style" => VirtualNodeKind::Style { index },
            _ => VirtualNodeKind::Custom { index },
        })
    }
}

pub fn resolve_external(owner: &str, specifier: &str) -> String {
    if specifier.starts_with('/') {
        return canonicalize_id(specifier).into_owned();
    }
    if specifier.starts_with('.') {
        // Relative branch delegates to the workspace's single source of
        // truth for path-join semantics. Algorithm preserved byte-for-byte
        // (parent-pop guard, root-segment guard, "//"/"./" elision).
        return verter_workspace::relative_path::join_relative(owner, specifier);
    }
    canonicalize_id(specifier).into_owned()
}

/// The language a node's ids name, as the carrier's metadata records it.
fn meta_lang<'m>(node: &VirtualNodeKind, meta: &'m FileMeta) -> Option<&'m str> {
    match node {
        VirtualNodeKind::Script => meta.script_lang.as_deref(),
        VirtualNodeKind::Style { index } => meta.style_langs.get(*index)?.as_deref(),
        VirtualNodeKind::Main | VirtualNodeKind::Template | VirtualNodeKind::Custom { .. } => None,
    }
}

/// The block type of a custom node, as the carrier's metadata records it.
fn meta_block_type<'m>(node: &VirtualNodeKind, meta: &'m FileMeta) -> Option<&'m str> {
    match node {
        VirtualNodeKind::Custom { index } => meta.custom_types.get(*index).map(String::as_str),
        _ => None,
    }
}

/// Render the bundler and LSP ids of a node of a carrier the host holds.
pub(crate) fn render_ids(
    canonical_id: &str,
    node: &VirtualNodeKind,
    meta: &FileMeta,
) -> (String, String) {
    render_node_ids(
        canonical_id,
        node,
        meta.request_marker,
        meta_lang(node, meta),
        meta_block_type(node, meta),
    )
}

/// Render the ids of a resolved request: in the marker, language and block
/// type the host's metadata records when the host holds the carrier, else
/// in the marker of the language the host classifies the carrier as and the
/// language and block type the request names.
pub(crate) fn render_resolved_ids(
    canonical_id: &str,
    parsed: &ParsedRawId,
    held_meta: Option<&FileMeta>,
    classified_marker: RequestMarker,
) -> (String, String) {
    match held_meta {
        Some(meta) => render_ids(canonical_id, &parsed.node_kind, meta),
        None => render_node_ids(
            canonical_id,
            &parsed.node_kind,
            classified_marker,
            parsed.lang.as_deref(),
            parsed.block_type.as_deref(),
        ),
    }
}

/// A script without a `lang` is JavaScript, a style without one is CSS, and
/// a custom block without a type is `custom`.
fn render_node_ids(
    canonical_id: &str,
    node: &VirtualNodeKind,
    marker: RequestMarker,
    lang: Option<&str>,
    block_type: Option<&str>,
) -> (String, String) {
    let marker = marker.as_str();
    match node {
        VirtualNodeKind::Main => (
            canonical_id.to_string(),
            format!("{}._VERTER_.bundle.ts", canonical_id),
        ),
        VirtualNodeKind::Script => (
            format!(
                "{}?{}&type=script&lang.{}",
                canonical_id,
                marker,
                lang.unwrap_or("js")
            ),
            format!("{}._VERTER_.options.ts", canonical_id),
        ),
        VirtualNodeKind::Template => (
            format!("{}?{}&type=template", canonical_id, marker),
            format!("{}._VERTER_.render.tsx", canonical_id),
        ),
        VirtualNodeKind::Style { index } => {
            let lang = lang.unwrap_or("css");
            (
                format!(
                    "{}?{}&type=style&index={}&lang.{}",
                    canonical_id, marker, index, lang
                ),
                format!("{}._VERTER_.style.{}.{}", canonical_id, index, lang),
            )
        }
        VirtualNodeKind::Custom { index } => {
            let block_type = block_type.unwrap_or("custom");
            (
                format!(
                    "{}?{}&type={}&index={}",
                    canonical_id, marker, block_type, index
                ),
                format!("{}._VERTER_.custom.{}.{}", canonical_id, index, block_type),
            )
        }
    }
}

impl crate::VerterHost {
    /// The metadata of a carrier this host holds.
    pub(crate) fn held_file_meta(&self, canonical_id: &str) -> Option<FileMeta> {
        use crate::host_executor::HostSourceData;
        self.scheduler
            .try_get_source(canonical_id)
            .and_then(|source| {
                source
                    .downcast_data::<HostSourceData>()
                    .map(|held| held.parse.meta.clone())
            })
    }

    /// The marker of a carrier this host does not hold: from the language
    /// this host classifies it as, its carrier suffixes included.
    pub(crate) fn classified_marker(&self, canonical_id: &str) -> RequestMarker {
        RequestMarker::of_language(&self.language_classifier().classify(canonical_id))
    }

    /// The bundler id of `node` of a carrier, in `lang` (which may be dotted,
    /// as in `module.css`) when given. The marker, the custom block type and,
    /// without `lang`, the language come from the host's metadata when it
    /// holds the carrier; otherwise the marker comes from the language this
    /// host classifies the carrier as.
    pub fn render_bundler_id(
        &self,
        canonical_id: &str,
        node: &VirtualNodeKind,
        lang: Option<&str>,
    ) -> String {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let held = self.held_file_meta(&canonical);
        let (marker, held_lang, block_type) = match &held {
            Some(meta) => (
                meta.request_marker,
                meta_lang(node, meta),
                meta_block_type(node, meta),
            ),
            None => (self.classified_marker(&canonical), None, None),
        };
        render_node_ids(&canonical, node, marker, lang.or(held_lang), block_type).0
    }
}

pub(crate) fn render_single_id(
    canonical_id: &str,
    node: &VirtualNodeKind,
    meta: &FileMeta,
    lsp: bool,
) -> String {
    let (bundler, lsp_id) = render_ids(canonical_id, node, meta);
    if lsp {
        lsp_id
    } else {
        bundler
    }
}

#[cfg(test)]
#[path = "id_tests.rs"]
mod tests;
