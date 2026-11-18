#![allow(dead_code)]
//! External `src` blocks bound through host-owned source identities.

use oxc_sourcemap::SourceMap;
use verter_css_syntax::PreprocessorIdentity;
use verter_identity::encoding::CanonicalEncoder;

use crate::code_transform::{CodeTransform, SourceMapChainError};
use crate::framework_common::projection_plan::origin::compose_source_chain;
use crate::ide::vue_projection::script_setup::SourceRange;

/// Which block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalBlockKind {
    Script,
    Template,
    Style,
}

/// Whether an external block may be projected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalLegality {
    Admitted,
    /// `<script setup src>`: not legal Vue.
    ScriptSetupSrc,
    /// The external file's dialect differs from the sibling block's.
    DialectMismatch,
    /// A preprocessed language the host did not process.
    UnsupportedLanguage(String),
    /// The host could not read the source.
    Unreadable,
}

/// One step of processing between authored bytes and the bytes projected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChainLink {
    pub from_space: String,
    pub to_space: String,
    /// Source map of this step; `None` for identity.
    pub map: Option<String>,
    /// True when the map leaves spans unmapped. Those spans get no source
    /// position; none is invented.
    pub has_holes: bool,
}

/// Authored source to projected bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessedSourceChain {
    pub links: Vec<SourceChainLink>,
}

/// What the host returned for one external block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundExternalBlock {
    pub kind: ExternalBlockKind,
    /// Authored `src` value and its span.
    pub specifier: String,
    pub specifier_range: crate::ide::vue_projection::script_setup::SourceRange,
    /// Host-owned identity of the external source; two carriers naming one
    /// file get the same identity.
    pub source_identity: String,
    pub legality: ExternalLegality,
    pub chain: ProcessedSourceChain,
}

/// Everything the projection of one carrier read outside the carrier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalDependencyReadSet {
    pub source_identities: Vec<String>,
    /// Digest of the preprocessing configuration that produced the chains.
    pub configuration_digest: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalBlockProjectionBinding {
    pub blocks: Vec<BoundExternalBlock>,
    pub read_set: ExternalDependencyReadSet,
}

/// Host side of external blocks. The compiler never reads files.
pub trait ExternalSourceHost {
    /// Resolve and describe `specifier` as authored in `importer`.
    fn bind(&self, importer: &str, kind: ExternalBlockKind, specifier: &str) -> BoundExternalBlock;
}

/// Bind every `src` block of an admitted parse.
///
/// The links (block kind, raw specifier, specifier span, script role) come
/// from the registered carrier inventory through the parse artifact; the
/// compiler never reads files or resolves the specifier. The host answers
/// with the source identity, the processed chain and the legality it can
/// judge (dialect, preprocessed languages, readability). The carrier owns
/// kind, specifier and range, and `<script setup src>` is refused here
/// because the role is carrier structure. An empty `src` value names no
/// external file.
pub fn bind_external_blocks(
    _source: &str,
    artifact: &crate::framework_common::FrameworkParseArtifact,
    canonical_id: &str,
    host: &dyn ExternalSourceHost,
) -> ExternalBlockProjectionBinding {
    use verter_language::{ExternalLinkKind, ScriptRole};

    let mut binding = ExternalBlockProjectionBinding::default();
    for link in artifact.common().external_links() {
        let (Some(span), false) = (link.span, link.specifier.is_empty()) else {
            continue;
        };
        let kind = match link.kind {
            ExternalLinkKind::Script => ExternalBlockKind::Script,
            ExternalLinkKind::Template => ExternalBlockKind::Template,
            ExternalLinkKind::Style => ExternalBlockKind::Style,
        };
        let mut bound = host.bind(canonical_id, kind, &link.specifier);
        bound.kind = kind;
        bound.specifier = link.specifier;
        bound.specifier_range = SourceRange {
            start: span.start,
            end: span.end,
        };
        if link.script_role == Some(ScriptRole::Setup) {
            bound.legality = ExternalLegality::ScriptSetupSrc;
        }
        if !binding
            .read_set
            .source_identities
            .contains(&bound.source_identity)
        {
            binding
                .read_set
                .source_identities
                .push(bound.source_identity.clone());
        }
        binding.blocks.push(bound);
    }
    binding
}

impl ExternalDependencyReadSet {
    /// Record the preprocessing configuration the chains were produced under.
    /// The identities are the host's own ([`PreprocessorIdentity`], the same
    /// vocabulary block content is admitted with), so a changed tool, version
    /// or configuration fingerprint changes the read set even when no source
    /// text did.
    pub fn bind_configuration(&mut self, producers: &[PreprocessorIdentity]) {
        self.configuration_digest = configuration_digest(producers);
    }
}

/// Digest of the preprocessing configuration named by `producers`, in order.
/// `None` when nothing was preprocessed, so adding a preprocessor is a change.
#[must_use]
pub fn configuration_digest(producers: &[PreprocessorIdentity]) -> Option<[u8; 32]> {
    if producers.is_empty() {
        return None;
    }
    let mut encoder = CanonicalEncoder::new("verter.vue-projection.external-configuration.v1");
    encoder.field_u64(1, producers.len() as u64);
    for producer in producers {
        match producer {
            PreprocessorIdentity::Anonymous => {
                encoder.field_enum_discriminant(2, 0);
            }
            PreprocessorIdentity::Named(named) => {
                encoder
                    .field_enum_discriminant(2, 1)
                    .field_str(3, named.identity())
                    .field_option(4, named.version().map(str::as_bytes))
                    .field_option(5, named.config_fingerprint().map(str::as_bytes));
            }
        }
    }
    Some(*encoder.digest().as_bytes())
}

/// Why a chain cannot be expressed against the authored external source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalChainRefusal {
    /// More than one link carries a source map. Re-expressing one map through
    /// a transform is the only composition available, so stacked
    /// preprocessors are refused rather than approximated.
    SeveralMappedLinks { mapped: usize },
    /// The link at this index carries a map that is not a source map.
    InvalidMap { link: usize },
    /// The transform that owns the projected bytes has no chaining semantics.
    Transform(SourceMapChainError),
}

/// A position in the authored external source, in the source map's own
/// coordinates (zero-based line and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoredPosition {
    pub line: u32,
    pub column: u32,
}

/// Projected bytes of an external block against its authored source.
#[derive(Debug, Clone)]
pub struct AuthoredSourceMap {
    map: SourceMap<'static>,
}

impl AuthoredSourceMap {
    /// The map itself, projected bytes to authored source.
    #[must_use]
    pub fn map(&self) -> &SourceMap<'static> {
        &self.map
    }

    /// The authored position of each projected `(line, column)`, in order.
    /// A position the map leaves unmapped, whether its segment is sourceless
    /// or its line has none, gets `None`: no position is invented for it.
    #[must_use]
    pub fn locate(&self, projected: &[(u32, u32)]) -> Vec<Option<AuthoredPosition>> {
        let table = self.map.generate_lookup_table();
        projected
            .iter()
            .map(|&(line, column)| {
                let token = self.map.lookup_token(&table, line, column)?;
                token.get_source_id()?;
                Some(AuthoredPosition {
                    line: token.get_src_line(),
                    column: token.get_src_col(),
                })
            })
            .collect()
    }
}

impl ProcessedSourceChain {
    /// The map of the bytes `transform` produces against the authored
    /// external source: the chain's one mapped link, re-expressed through the
    /// transform. `None` when no link carries a map, because the projected
    /// bytes are then the authored bytes and the transform's own mapping is
    /// the answer.
    ///
    /// # Errors
    ///
    /// [`ExternalChainRefusal`] when the chain stacks several maps, a map is
    /// malformed, or the transform has no chaining semantics.
    pub fn authored_map(
        &self,
        transform: &CodeTransform<'_, impl verter_audit::observe::ObserveExec>,
    ) -> Result<Option<AuthoredSourceMap>, ExternalChainRefusal> {
        let mut mapped = self
            .links
            .iter()
            .enumerate()
            .filter_map(|(index, link)| link.map.as_deref().map(|map| (index, map)));
        let Some((link, json)) = mapped.next() else {
            return Ok(None);
        };
        let others = mapped.count();
        if others > 0 {
            return Err(ExternalChainRefusal::SeveralMappedLinks { mapped: others + 1 });
        }
        let upstream = SourceMap::from_json_string(json)
            .map_err(|_| ExternalChainRefusal::InvalidMap { link })?;
        let map =
            compose_source_chain(transform, &upstream).map_err(ExternalChainRefusal::Transform)?;
        Ok(Some(AuthoredSourceMap { map }))
    }
}
