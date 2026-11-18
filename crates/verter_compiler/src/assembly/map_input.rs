//! Fragment input-map validation and the exhaustive rejection taxonomy.
//!
//! Missing, malformed, or uncomposable required mapping is fail-closed —
//! never an empty, approximate, or unmapped success. Validation finishes
//! before any composition. One outcome per rejection: checks are totally
//! ordered; the first failure wins.

use super::map_json::{parse, Json};

/// Which fragment an outcome is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapFragment {
    Script,
    Template,
}

impl MapFragment {
    pub fn as_str(self) -> &'static str {
        match self {
            MapFragment::Script => "script",
            MapFragment::Template => "template",
        }
    }
}

/// The eight ratified families of structurally unusable input map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UncomposableFamily {
    /// Malformed map JSON.
    MalformedJson,
    /// Wrong or missing version.
    Version,
    /// Undecodable or out-of-range wire data.
    WireData,
    /// Malformed table rows.
    TableRows,
    /// An indexed (non-flat) map.
    IndexedMap,
    /// A dangling table index.
    DanglingIndex,
    /// An out-of-fragment or surrogate-split coordinate.
    Coordinate,
    /// Incompatible cross-fragment table metadata.
    CrossFragmentMetadata,
}

/// The exact sub-code of an uncomposable input map. Every code belongs to
/// exactly one family; an input passing every code is composable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UncomposableCode {
    /// The `sourceMap` string is not parseable JSON.
    MapBytesNotJson,
    /// The parsed root is not an object.
    MapRootNotObject,
    /// No `mappings` member. An absent `mappings` is never read as an empty map.
    MappingsMemberAbsent,
    /// `mappings` present but not a string.
    MappingsMemberNotAString,
    /// `sources` absent, or present and not an array.
    SourcesMemberAbsentOrNotAnArray,
    /// `names` absent, or present and not an array.
    NamesMemberAbsentOrNotAnArray,
    /// A metadata member has the wrong type, or two ignore-list spellings
    /// disagree.
    MetadataMemberWrongType,
    /// Some JSON object declares the same member name twice.
    DuplicateObjectMember,
    /// A JSON number does not denote a finite IEEE-754 double.
    NumberOutsideInteroperableDomain,
    /// A JSON string contains an unpaired surrogate after unescaping.
    StringNotWellFormedUnicode,
    /// No `version` member.
    VersionMemberAbsent,
    /// `version` present but not an integral JSON number.
    VersionNotAnInteger,
    /// `version` is an integer other than 3.
    VersionNot3,
    /// A `mappings` character outside the base64 alphabet inside a segment.
    VlqInvalidCharacter,
    /// A segment ends while a continuation bit is set.
    VlqTruncatedSegment,
    /// A decoded segment has a field count other than 1, 4 or 5.
    SegmentFieldCount,
    /// A field's encoding continues past bit 31, or its value falls outside
    /// `[-(2^31-1), 2^31-1]`.
    VlqFieldOutOfRange,
    /// A running accumulator became negative or exceeded `2^31-1`.
    AccumulatorOutOfRange,
    /// Within one generated line, a segment's column is strictly less than the
    /// previous segment's.
    GeneratedColumnAccumulatorDecreased,
    /// A `sources` element is not a string.
    SourceRowNotAString,
    /// A `names` element is not a string.
    NameRowNotAString,
    /// A `sourcesContent` element is neither a string nor null.
    SourcesContentRowNotStringOrNull,
    /// `sourcesContent` is present and its length differs from `sources`.
    SourcesContentLengthMismatch,
    /// A `sections` member is present, with any value.
    SectionsMemberPresent,
    /// A segment's non-null source index is not in `[0, sources.length)`.
    SourceIndexOutOfTable,
    /// A segment's non-null name index is not in `[0, names.length)`.
    NameIndexOutOfTable,
    /// An ignore-list entry is not in `[0, sources.length)`.
    IgnoreListIndexOutOfTable,
    /// A generated line is not in `[0, lineTable(code).length)`.
    GeneratedLineOutOfFragment,
    /// A generated column is not in `[0, lineTable(code)[line].length]`.
    GeneratedColumnOutOfFragment,
    /// A generated column addresses no character boundary because it falls
    /// between the two halves of a surrogate pair.
    GeneratedColumnSplitsASurrogatePair,
    /// Two contributing maps declare different normalised `sourceRoot` values.
    SourceRootConflict,
}

impl UncomposableCode {
    pub fn family(self) -> UncomposableFamily {
        use UncomposableCode as C;
        use UncomposableFamily as F;
        match self {
            C::MapBytesNotJson
            | C::MapRootNotObject
            | C::MappingsMemberAbsent
            | C::MappingsMemberNotAString
            | C::SourcesMemberAbsentOrNotAnArray
            | C::NamesMemberAbsentOrNotAnArray
            | C::MetadataMemberWrongType
            | C::DuplicateObjectMember
            | C::NumberOutsideInteroperableDomain
            | C::StringNotWellFormedUnicode => F::MalformedJson,
            C::VersionMemberAbsent | C::VersionNotAnInteger | C::VersionNot3 => F::Version,
            C::VlqInvalidCharacter
            | C::VlqTruncatedSegment
            | C::SegmentFieldCount
            | C::VlqFieldOutOfRange
            | C::AccumulatorOutOfRange
            | C::GeneratedColumnAccumulatorDecreased => F::WireData,
            C::SourceRowNotAString
            | C::NameRowNotAString
            | C::SourcesContentRowNotStringOrNull
            | C::SourcesContentLengthMismatch => F::TableRows,
            C::SectionsMemberPresent => F::IndexedMap,
            C::SourceIndexOutOfTable | C::NameIndexOutOfTable | C::IgnoreListIndexOutOfTable => {
                F::DanglingIndex
            }
            C::GeneratedLineOutOfFragment
            | C::GeneratedColumnOutOfFragment
            | C::GeneratedColumnSplitsASurrogatePair => F::Coordinate,
            C::SourceRootConflict => F::CrossFragmentMetadata,
        }
    }

    /// The stable diagnostic spelling, `"<family>.<index> <kebab-name>"`.
    pub fn as_str(self) -> &'static str {
        use UncomposableCode as C;
        match self {
            C::MapBytesNotJson => "U1.1 map-bytes-not-json",
            C::MapRootNotObject => "U1.2 map-root-not-object",
            C::MappingsMemberAbsent => "U1.3 mappings-member-absent",
            C::MappingsMemberNotAString => "U1.4 mappings-member-not-a-string",
            C::SourcesMemberAbsentOrNotAnArray => "U1.5 sources-member-absent-or-not-an-array",
            C::NamesMemberAbsentOrNotAnArray => "U1.6 names-member-absent-or-not-an-array",
            C::MetadataMemberWrongType => "U1.7 metadata-member-wrong-type",
            C::DuplicateObjectMember => "U1.8 duplicate-object-member",
            C::NumberOutsideInteroperableDomain => "U1.9 number-outside-interoperable-domain",
            C::StringNotWellFormedUnicode => "U1.10 string-not-well-formed-unicode",
            C::VersionMemberAbsent => "U2.1 version-member-absent",
            C::VersionNotAnInteger => "U2.2 version-not-an-integer",
            C::VersionNot3 => "U2.3 version-not-3",
            C::VlqInvalidCharacter => "U3.1 vlq-invalid-character",
            C::VlqTruncatedSegment => "U3.2 vlq-truncated-segment",
            C::SegmentFieldCount => "U3.3 segment-field-count",
            C::VlqFieldOutOfRange => "U3.4 vlq-field-out-of-range",
            C::AccumulatorOutOfRange => "U3.5 accumulator-out-of-range",
            C::GeneratedColumnAccumulatorDecreased => "U3.6 generated-column-accumulator-decreased",
            C::SourceRowNotAString => "U4.1 source-row-not-a-string",
            C::NameRowNotAString => "U4.2 name-row-not-a-string",
            C::SourcesContentRowNotStringOrNull => "U4.3 sources-content-row-not-string-or-null",
            C::SourcesContentLengthMismatch => "U4.4 sources-content-length-mismatch",
            C::SectionsMemberPresent => "U5.1 sections-member-present",
            C::SourceIndexOutOfTable => "U6.1 source-index-out-of-table",
            C::NameIndexOutOfTable => "U6.2 name-index-out-of-table",
            C::IgnoreListIndexOutOfTable => "U6.3 ignore-list-index-out-of-table",
            C::GeneratedLineOutOfFragment => "U7.1 generated-line-out-of-fragment",
            C::GeneratedColumnOutOfFragment => "U7.2 generated-column-out-of-fragment",
            C::GeneratedColumnSplitsASurrogatePair => {
                "U7.3 generated-column-splits-a-surrogate-pair"
            }
            C::SourceRootConflict => "U8.1 source-root-conflict",
        }
    }
}

/// Why assembly produced no result. Every variant is a hard failure — never
/// code without a map, never code with an empty map, never a rewrite applied
/// from a fact that does not match the script's own bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssembleMapFailure {
    /// A fragment that is both authored and present carries an empty map.
    /// Deliberately NOT one of the eight uncomposable families: a missing map
    /// and an uncomposable map are separate triggers with separate owners.
    MissingRequiredInputMap { fragment: MapFragment },
    /// A present map is structurally unusable.
    UncomposableInputMap {
        fragment: MapFragment,
        code: UncomposableCode,
    },
    /// The script's declared `__sfc__` export-placement fact
    /// (`verter_compiler::assembly::fragment::SfcExportPlacement`) is out
    /// of bounds or does not match the script's own bytes — a producer
    /// defect, reported rather than silently rediscovered by scanning
    /// generated text for the landmark string.
    InvalidSfcExportPlacement { reason: super::SfcRewriteRefusal },
}

impl AssembleMapFailure {
    pub fn fragment(&self) -> Option<MapFragment> {
        match self {
            AssembleMapFailure::MissingRequiredInputMap { fragment }
            | AssembleMapFailure::UncomposableInputMap { fragment, .. } => Some(*fragment),
            AssembleMapFailure::InvalidSfcExportPlacement { .. } => None,
        }
    }
}

impl std::fmt::Display for AssembleMapFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssembleMapFailure::MissingRequiredInputMap { fragment } => write!(
                f,
                "the {} fragment is authored and present but carries no source map",
                fragment.as_str()
            ),
            AssembleMapFailure::UncomposableInputMap { fragment, code } => write!(
                f,
                "the {} fragment's source map is uncomposable: {}",
                fragment.as_str(),
                code.as_str()
            ),
            AssembleMapFailure::InvalidSfcExportPlacement { reason } => write!(
                f,
                "the script's declared __sfc__ export-placement fact is invalid: {reason:?}"
            ),
        }
    }
}

impl std::error::Error for AssembleMapFailure {}

// The decoded map

/// A segment's authored payload. Absent for a sourceless segment, whose four
/// authored fields are all null by definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePayload {
    pub source_index: u32,
    pub source_line: u32,
    pub source_column: u32,
    pub name_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireSegment {
    pub generated_line: u32,
    pub generated_column: u32,
    pub payload: Option<SourcePayload>,
}

/// One fragment's validated map, ready to compose.
///
/// Fields are private so public callers cannot mint an unvalidated
/// instance that [`super::map_compose::to_source_map`] would trust.
/// Construct only through [`validate_and_decode`].
#[derive(Debug, Clone)]
pub struct DecodedFragmentMap {
    sources: Vec<String>,
    names: Vec<String>,
    /// Absent when the input declared no `sourcesContent`; otherwise parallel
    /// to `sources`.
    sources_content: Option<Vec<Option<String>>>,
    /// Normalised: absent when the member is absent or JSON null.
    source_root: Option<String>,
    /// The validated entries at full binary64 identity — non-negative,
    /// integral, and proven in `[0, sources.len())`, so a consumer may narrow
    /// to a small integer type; the wide storage exists because every numeric
    /// predicate BEFORE the bounds proof (type check, two-spelling agreement)
    /// operates on the converted binary64 value, which an integer pre-narrow
    /// would corrupt (distinct values ≥ 2^64 saturate to one). Read by
    /// `map_compose::to_source_map` when chaining the script's map through
    /// the `__sfc__` rewrite — the template's copy is otherwise unread by
    /// composition, which sequences the template's RAW already-encoded map
    /// string directly (`assemble_sequence` decodes it, and this field's
    /// ignore-list bounds, again, independently).
    ignore_list: Vec<f64>,
    segments: Vec<WireSegment>,
    /// Identity of the decoded tables. Parts sliced from one map share it,
    /// so an artifact that writes them carries those tables once.
    tables: TablesId,
}

/// Identity of one decoded map's tables, unique in the process.
#[derive(Debug, Clone, Copy, Eq)]
pub(crate) struct TablesId(u64);

impl TablesId {
    fn fresh() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

impl PartialEq for TablesId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl DecodedFragmentMap {
    /// Identity of this map's tables; slices of one map share it.
    pub(crate) fn tables(&self) -> TablesId {
        self.tables
    }

    #[must_use]
    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    #[must_use]
    pub fn sources_content(&self) -> Option<&[Option<String>]> {
        self.sources_content.as_deref()
    }

    #[must_use]
    pub fn source_root(&self) -> Option<&str> {
        self.source_root.as_deref()
    }

    #[must_use]
    pub fn ignore_list(&self) -> &[f64] {
        &self.ignore_list
    }

    #[must_use]
    pub fn segments(&self) -> &[WireSegment] {
        &self.segments
    }

    /// A map from a crate-internal producer whose segments are valid by
    /// construction: every index lies in its table and every generated
    /// position lies in `fragment_code`. Debug builds check the same bounds
    /// [`validate_and_decode`] enforces.
    pub(crate) fn from_segments(
        fragment_code: &str,
        sources: Vec<String>,
        sources_content: Option<Vec<Option<String>>>,
        names: Vec<String>,
        segments: Vec<WireSegment>,
    ) -> Self {
        verter_debug_assert!(
            !cfg!(debug_assertions)
                || (sources_content
                    .as_ref()
                    .is_none_or(|rows| rows.len() == sources.len())
                    && check_table_indices(&segments, sources.len(), names.len()).is_ok()
                    && check_generated_positions(&segments, fragment_code).is_ok()),
            "a crate-built fragment map stays inside its tables and its code"
        );
        Self {
            sources,
            names,
            sources_content,
            source_root: None,
            ignore_list: Vec::new(),
            segments,
            tables: TablesId::fresh(),
        }
    }

    /// The part of this map, decoded against `code`, that describes the
    /// bytes from `start` on: its segments at or after that position (and
    /// before `end`, when one is given), placed relative to it. The tables
    /// are kept whole. `None` when a bound is not a position of `code`.
    pub(crate) fn slice(&self, code: &str, start: u32, end: Option<u32>) -> Option<Self> {
        let origin = generated_position(code, start)?;
        let end = match end {
            Some(end) => Some(generated_position(code, end)?),
            None => None,
        };
        let segments = self
            .segments
            .iter()
            .filter(|segment| {
                let at = GeneratedPosition {
                    line: segment.generated_line,
                    column: segment.generated_column,
                };
                at >= origin && end.is_none_or(|end| at < end)
            })
            .map(|segment| WireSegment {
                generated_line: segment.generated_line - origin.line,
                generated_column: if segment.generated_line == origin.line {
                    segment.generated_column - origin.column
                } else {
                    segment.generated_column
                },
                ..*segment
            })
            .collect();
        Some(Self {
            sources: self.sources.clone(),
            names: self.names.clone(),
            sources_content: self.sources_content.clone(),
            source_root: self.source_root.clone(),
            ignore_list: self.ignore_list.clone(),
            segments,
            tables: self.tables,
        })
    }

    /// The same segments over one source row named `name` with `content`:
    /// every mapped segment names that row, and names are dropped.
    pub(crate) fn as_single_source(&self, name: &str, content: &str) -> Self {
        Self {
            sources: vec![name.to_owned()],
            names: Vec::new(),
            sources_content: Some(vec![Some(content.to_owned())]),
            source_root: None,
            ignore_list: Vec::new(),
            tables: TablesId::fresh(),
            segments: self
                .segments
                .iter()
                .map(|segment| WireSegment {
                    payload: segment.payload.map(|payload| SourcePayload {
                        source_index: 0,
                        name_index: None,
                        ..payload
                    }),
                    ..*segment
                })
                .collect(),
        }
    }

    /// The same map with `source_root` and the ignore-listed rows of its
    /// source table. Debug builds check that every entry names a row.
    pub(crate) fn with_layout(
        mut self,
        source_root: Option<String>,
        ignore_list: Vec<f64>,
    ) -> Self {
        verter_debug_assert!(
            ignore_list
                .iter()
                .all(|entry| *entry >= 0.0 && (*entry as usize) < self.sources.len()),
            "an ignore-list entry names a source row"
        );
        self.source_root = source_root;
        self.ignore_list = ignore_list;
        self
    }
}

/// A zero-based generated line and UTF-16 column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GeneratedPosition {
    pub(crate) line: u32,
    pub(crate) column: u32,
}

/// Generated position of byte `offset` in `code`, when the offset is a
/// position of it.
pub(crate) fn generated_position(code: &str, offset: u32) -> Option<GeneratedPosition> {
    let offset = usize::try_from(offset).ok()?;
    if offset > code.len() || !code.is_char_boundary(offset) {
        return None;
    }
    let (mut line, mut column) = (0, 0);
    crate::code_transform::advance_generated_position(&code[..offset], &mut line, &mut column);
    Some(GeneratedPosition { line, column })
}

/// Every source and name index lies in its table. A sourceless segment
/// carries no index.
fn check_table_indices(
    segments: &[WireSegment],
    sources: usize,
    names: usize,
) -> Result<(), UncomposableCode> {
    for segment in segments {
        if let Some(payload) = segment.payload {
            if payload.source_index as usize >= sources {
                return Err(UncomposableCode::SourceIndexOutOfTable);
            }
            if let Some(name_index) = payload.name_index {
                if name_index as usize >= names {
                    return Err(UncomposableCode::NameIndexOutOfTable);
                }
            }
        }
    }
    Ok(())
}

/// Every generated position is a position of `fragment_code`.
fn check_generated_positions(
    segments: &[WireSegment],
    fragment_code: &str,
) -> Result<(), UncomposableCode> {
    let lines: Vec<&str> = fragment_code.split('\n').collect();
    for segment in segments {
        let line = *lines
            .get(segment.generated_line as usize)
            .ok_or(UncomposableCode::GeneratedLineOutOfFragment)?;
        match classify_column(line, segment.generated_column) {
            ColumnKind::InBounds => {}
            ColumnKind::OutOfBounds => {
                return Err(UncomposableCode::GeneratedColumnOutOfFragment);
            }
            ColumnKind::SplitsSurrogatePair => {
                return Err(UncomposableCode::GeneratedColumnSplitsASurrogatePair);
            }
        }
    }
    Ok(())
}

const I32_MAX: i64 = 2_147_483_647;

#[cfg(test)]
thread_local! {
    static JSON_MAP_DECODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Encoded maps decoded on this thread since the last call; resets the
/// count.
#[cfg(test)]
pub(crate) fn take_json_map_decodes() -> usize {
    JSON_MAP_DECODES.with(|decodes| decodes.replace(0))
}

/// Validate and decode one contributing map against the fragment's own,
/// PRE-REWRITE code, in the specified total order.
pub fn validate_and_decode(
    raw: &str,
    fragment_code: &str,
) -> Result<DecodedFragmentMap, UncomposableCode> {
    use UncomposableCode as C;

    #[cfg(test)]
    JSON_MAP_DECODES.with(|decodes| decodes.set(decodes.get() + 1));

    // 1.1 — the bytes are an admissible JSON document. Three ordered clauses,
    // first failure wins, all three BEFORE any member is read, so the outcome
    // does not depend on which one an implementation happens to notice first.
    let document = parse(raw).map_err(|()| C::MapBytesNotJson)?;
    if document.first_non_finite_number().is_some() {
        return Err(C::NumberOutsideInteroperableDomain);
    }
    if document.has_ill_formed_string() {
        return Err(C::StringNotWellFormedUnicode);
    }

    // 1.2 — duplicate-member detection precedes every member read, so no later
    // check can silently read whichever duplicate a parser happened to keep.
    if document.has_duplicate_member() {
        return Err(C::DuplicateObjectMember);
    }

    // 1.3
    if !matches!(document, Json::Object(_)) {
        return Err(C::MapRootNotObject);
    }

    // 1.4 – 1.6 — version beats indexed-map, so a `version: 2` map that also
    // carries `sections` reports the version.
    let version = document.member("version").ok_or(C::VersionMemberAbsent)?;
    let version = version.as_number().ok_or(C::VersionNotAnInteger)?;
    if version.fract() != 0.0 {
        return Err(C::VersionNotAnInteger);
    }
    if version != 3.0 {
        return Err(C::VersionNot3);
    }

    // 1.7 — indexed-map beats missing `mappings`: an indexed map legitimately
    // has none.
    if document.member("sections").is_some() {
        return Err(C::SectionsMemberPresent);
    }

    // 1.8 – 1.9
    let mappings = document.member("mappings").ok_or(C::MappingsMemberAbsent)?;
    let mappings = mappings.as_str().ok_or(C::MappingsMemberNotAString)?;

    // 1.10 – 1.11
    let sources = document
        .member("sources")
        .and_then(Json::as_array)
        .ok_or(C::SourcesMemberAbsentOrNotAnArray)?;
    let names = document
        .member("names")
        .and_then(Json::as_array)
        .ok_or(C::NamesMemberAbsentOrNotAnArray)?;

    // 1.12 – 1.16 — metadata shape.
    let sources_content = match document.member("sourcesContent") {
        None => None,
        Some(value) => Some(value.as_array().ok_or(C::MetadataMemberWrongType)?),
    };
    let source_root = match document.member("sourceRoot") {
        None => None,
        Some(value) if value.is_null() => None,
        Some(value) => Some(value.as_str().ok_or(C::MetadataMemberWrongType)?.to_owned()),
    };
    if let Some(file) = document.member("file") {
        if !file.is_null() && file.as_str().is_none() {
            return Err(C::MetadataMemberWrongType);
        }
    }
    let ignore_list = read_ignore_list(&document)?;
    if let Some(debug_id) = document.member("debugId") {
        if debug_id.as_str().is_none() {
            return Err(C::MetadataMemberWrongType);
        }
    }

    // 1.17 – 1.20 — row typing beats wire decoding, because index-bounds and
    // coordinate checks presuppose a typed table. `sources` rows beat `names`
    // rows beat `sourcesContent` rows.
    let mut source_rows = Vec::with_capacity(sources.len());
    for row in sources {
        source_rows.push(row.as_str().ok_or(C::SourceRowNotAString)?.to_owned());
    }
    let mut name_rows = Vec::with_capacity(names.len());
    for row in names {
        name_rows.push(row.as_str().ok_or(C::NameRowNotAString)?.to_owned());
    }
    let content_rows = match sources_content {
        None => None,
        Some(rows) => {
            let mut decoded = Vec::with_capacity(rows.len());
            for row in rows {
                if row.is_null() {
                    decoded.push(None);
                } else {
                    decoded.push(Some(
                        row.as_str()
                            .ok_or(C::SourcesContentRowNotStringOrNull)?
                            .to_owned(),
                    ));
                }
            }
            if decoded.len() != source_rows.len() {
                return Err(C::SourcesContentLengthMismatch);
            }
            Some(decoded)
        }
    };

    // 1.21
    let segments = decode_mappings(mappings)?;

    // 1.22 — index bounds beat coordinate bounds, as a STAGE precedence: an
    // index violation in a later segment still beats a coordinate violation in
    // an earlier one. Both checks are guarded on the field being non-null,
    // because a sourceless segment carries null in every authored field and
    // null is in no index range.
    check_table_indices(&segments, source_rows.len(), name_rows.len())?;

    // 1.23 — `entry` is a validated finite, non-negative, integral binary64
    // value with no upper bound imposed at step 1.15 (unlike a VLQ-decoded
    // segment field, an ignore-list entry is a plain JSON number), so the
    // bound is tested directly at binary64: `len as f64` is exact for any
    // real table (far below 2^53), and an entry at or beyond it — including
    // one too large for any machine integer — is out of table bounds.
    for entry in &ignore_list {
        if !(*entry >= 0.0 && *entry < source_rows.len() as f64) {
            return Err(C::IgnoreListIndexOutOfTable);
        }
    }

    // 1.24 — against the fragment's own code, in its own pre-rewrite space.
    check_generated_positions(&segments, fragment_code)?;

    Ok(DecodedFragmentMap {
        sources: source_rows,
        names: name_rows,
        sources_content: content_rows,
        source_root,
        ignore_list,
        segments,
        tables: TablesId::fresh(),
    })
}

/// Both accepted ignore-list spellings, which must agree when both appear. The
/// v3 field and its `x_google_` predecessor are one field, not two.
fn read_ignore_list(document: &Json) -> Result<Vec<f64>, UncomposableCode> {
    let standard = read_ignore_list_spelling(document.member("ignoreList"))?;
    let extension = read_ignore_list_spelling(document.member("x_google_ignoreList"))?;
    match (standard, extension) {
        // The agreement is over the CONVERTED binary64 values (exact f64
        // equality — every entry is a validated non-negative integral value,
        // never NaN), so two distinct entries beyond any integer type's range
        // still disagree here rather than colliding under a saturating narrow.
        (Some(a), Some(b)) if a != b => Err(UncomposableCode::MetadataMemberWrongType),
        (Some(list), _) | (None, Some(list)) => Ok(list),
        (None, None) => Ok(Vec::new()),
    }
}

fn read_ignore_list_spelling(member: Option<&Json>) -> Result<Option<Vec<f64>>, UncomposableCode> {
    let Some(value) = member else {
        return Ok(None);
    };
    let entries = value
        .as_array()
        .ok_or(UncomposableCode::MetadataMemberWrongType)?;
    let mut list = Vec::with_capacity(entries.len());
    for entry in entries {
        // Step 1.15: type is "non-negative integral" — no upper bound.
        // Table bound is step 1.23. An entry beyond i32::MAX is still
        // legally typed here so it can fail as out-of-range there, not
        // wrong-type. Check the converted binary64, not the lexeme.
        let number = entry
            .as_number()
            .ok_or(UncomposableCode::MetadataMemberWrongType)?;
        if number.fract() != 0.0 || number < 0.0 {
            return Err(UncomposableCode::MetadataMemberWrongType);
        }
        // Stored at full binary64 identity: the two-spelling agreement check
        // and the step-1.23 bound both operate on the converted binary64
        // value, so no narrowing happens until the bound is proven.
        list.push(number);
    }
    Ok(Some(list))
}

enum ColumnKind {
    InBounds,
    OutOfBounds,
    SplitsSurrogatePair,
}

/// Classify a 0-based UTF-16 column against one line's text. A column equal to
/// the line's length is in bounds and denotes end-of-line.
fn classify_column(line: &str, column: u32) -> ColumnKind {
    let mut units = 0u32;
    for character in line.chars() {
        if units == column {
            return ColumnKind::InBounds;
        }
        units += character.len_utf16() as u32;
        if units > column {
            // The column landed strictly inside this character, which only a
            // surrogate pair's two units make possible.
            return ColumnKind::SplitsSurrogatePair;
        }
    }
    if units == column {
        ColumnKind::InBounds
    } else {
        ColumnKind::OutOfBounds
    }
}

// The wire decoder

fn base64_digit(character: u8) -> Option<u32> {
    match character {
        b'A'..=b'Z' => Some(u32::from(character - b'A')),
        b'a'..=b'z' => Some(u32::from(character - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(character - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode one VLQ field, rejecting an encoding whose bits run past bit 31.
///
/// The accepted decoders are lenient here — `"A"` and `"ggggggE"` both yield 0,
/// because a 32-bit shift wraps — so only the conforming encoding is admitted.
fn decode_field(bytes: &[u8], at: &mut usize) -> Result<i64, UncomposableCode> {
    let mut shift = 0u32;
    let mut word = 0u64;
    loop {
        let Some(character) = bytes.get(*at).copied() else {
            // The segment ended while a continuation bit was set.
            return Err(UncomposableCode::VlqTruncatedSegment);
        };
        let digit = base64_digit(character).ok_or(UncomposableCode::VlqInvalidCharacter)?;
        *at += 1;

        let payload = u64::from(digit & 31);
        if shift >= 32 || (shift + 5 > 32 && payload >= (1u64 << (32 - shift))) {
            return Err(UncomposableCode::VlqFieldOutOfRange);
        }
        word |= payload << shift;
        shift += 5;

        if digit & 32 == 0 {
            break;
        }
    }
    let magnitude = (word >> 1) as i64;
    let value = if word & 1 == 1 { -magnitude } else { magnitude };
    if !(-I32_MAX..=I32_MAX).contains(&value) {
        return Err(UncomposableCode::VlqFieldOutOfRange);
    }
    Ok(value)
}

/// Decode `mappings` left-to-right; first violation wins. Per segment:
/// lexical/per-field, then arity, then accumulator. Arity beats every
/// accumulator property (a three-field segment has no interpretation).
/// Within accumulator application, range beats ordering.
fn decode_mappings(mappings: &str) -> Result<Vec<WireSegment>, UncomposableCode> {
    let mut segments = Vec::new();
    let mut source_index = 0i64;
    let mut source_line = 0i64;
    let mut source_column = 0i64;
    let mut name_index = 0i64;

    for (line_number, group) in mappings.split(';').enumerate() {
        let mut generated_column = 0i64;
        let mut previous_column: Option<i64> = None;
        if group.is_empty() {
            continue;
        }
        for piece in group.split(',') {
            // Lexical and per-field, as each field is read, in wire order.
            let bytes = piece.as_bytes();
            let mut at = 0usize;
            let mut fields = Vec::with_capacity(5);
            while at < bytes.len() {
                fields.push(decode_field(bytes, &mut at)?);
            }

            // Arity, once the segment has been read in full.
            if !matches!(fields.len(), 1 | 4 | 5) {
                return Err(UncomposableCode::SegmentFieldCount);
            }

            // Accumulator application, now that the arity is known legal.
            generated_column += fields[0];
            if !(0..=I32_MAX).contains(&generated_column) {
                return Err(UncomposableCode::AccumulatorOutOfRange);
            }
            if previous_column.is_some_and(|previous| generated_column < previous) {
                return Err(UncomposableCode::GeneratedColumnAccumulatorDecreased);
            }
            previous_column = Some(generated_column);

            let payload = if fields.len() >= 4 {
                source_index += fields[1];
                if !(0..=I32_MAX).contains(&source_index) {
                    return Err(UncomposableCode::AccumulatorOutOfRange);
                }
                source_line += fields[2];
                if !(0..=I32_MAX).contains(&source_line) {
                    return Err(UncomposableCode::AccumulatorOutOfRange);
                }
                source_column += fields[3];
                if !(0..=I32_MAX).contains(&source_column) {
                    return Err(UncomposableCode::AccumulatorOutOfRange);
                }
                let name = if fields.len() == 5 {
                    name_index += fields[4];
                    if !(0..=I32_MAX).contains(&name_index) {
                        return Err(UncomposableCode::AccumulatorOutOfRange);
                    }
                    Some(name_index as u32)
                } else {
                    None
                };
                Some(SourcePayload {
                    source_index: source_index as u32,
                    source_line: source_line as u32,
                    source_column: source_column as u32,
                    name_index: name,
                })
            } else {
                None
            };

            segments.push(WireSegment {
                generated_line: line_number as u32,
                generated_column: generated_column as u32,
                payload,
            });
        }
    }
    Ok(segments)
}

/// Agree `sourceRoot` across contributing maps. Dropping or folding it
/// would change declared source identities. `""` is distinct from absent.
/// Zero maps → absent; one map → that value.
pub(crate) fn agree_source_root<'a>(
    contributing: impl IntoIterator<Item = (MapFragment, &'a DecodedFragmentMap)>,
) -> Result<Option<String>, AssembleMapFailure> {
    let mut agreed: Option<Option<String>> = None;
    for (fragment, map) in contributing {
        match &agreed {
            None => agreed = Some(map.source_root().map(str::to_string)),
            Some(existing) if existing.as_deref() == map.source_root() => {}
            // Attributed to the fragment that INTRODUCED the disagreement — the
            // later one in fixed script-then-template order, per layer 1's
            // `DECISION` D-8 (§4.3 step 2.1). Under the current two-fragment
            // DTO this is always the template; D-8 does not generalise beyond
            // that.
            Some(_) => {
                return Err(AssembleMapFailure::UncomposableInputMap {
                    fragment,
                    code: UncomposableCode::SourceRootConflict,
                })
            }
        }
    }
    Ok(agreed.flatten())
}
