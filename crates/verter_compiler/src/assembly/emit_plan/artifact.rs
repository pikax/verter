//! Finalisation: a plan is written once into one buffer, and its decoded map
//! is built in the same pass. Written text is immutable.

use verter_audit::observe::ObserveExec;

use super::plan::{EmitPlan, EmitPlanError, FillHost, SlotBody};
use super::segment::EmissionSegment;
use super::{ArtifactBoundary, MapMode};
use crate::assembly::fragment::GrammarWitness;
use crate::assembly::map_compose;
use crate::assembly::map_input::{DecodedFragmentMap, WireSegment};
use crate::emission::walk::Fill;
use crate::emission::writer::{EmissionSink, ExternalTables, MapParts, MapSink, TextOnly, Writer};
use crate::emission::{DecodedMapSegment, MapSegmentKind, SourceKey};
use crate::work_ledger::{CompilerWorkLedger, WorkKind};

#[cfg(test)]
thread_local! {
    static HOST_REMAP_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn take_host_remap_lookups() -> usize {
    HOST_REMAP_LOOKUPS.with(|count| count.replace(0))
}

/// Name and content of one of an artifact's own sources, as the encoded map
/// should carry them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSource<'a> {
    pub name: &'a str,
    pub content: Option<&'a str>,
}

/// Decoded map of one written artifact. Encoded only at an external
/// artifact boundary, by [`DecodedMap::to_source_map`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedMap {
    /// Mapped runs over the artifact's own sources, in generated order.
    segments: Vec<DecodedMapSegment>,
    /// The tokens the encoded map carries, placed during the write.
    wire: Vec<WireSegment>,
    /// The artifact's own sources come first in the source table.
    own_sources: u32,
    /// Tables of external maps, appended after the own sources.
    external: Vec<ExternalTables>,
    /// Names and ignore-listed rows introduced by per-source host maps.
    host_names: Vec<String>,
    host_ignore_list: Vec<u32>,
}

impl DecodedMap {
    /// A map over one fragment decoded by the assembly decoder: its tokens as
    /// the encoded map carries them, and the byte runs they anchor.
    pub(crate) fn from_decoded_fragment(
        fragment: &DecodedFragmentMap,
        segments: Vec<DecodedMapSegment>,
    ) -> Self {
        Self {
            segments,
            wire: fragment.segments().to_vec(),
            own_sources: u32::try_from(fragment.sources().len()).unwrap_or(u32::MAX),
            external: Vec::new(),
            host_names: Vec::new(),
            host_ignore_list: Vec::new(),
        }
    }

    pub fn segments(&self) -> &[DecodedMapSegment] {
        &self.segments
    }

    /// Whether an external piece's admitted map answers for some bytes;
    /// [`Self::segments`] does not hold those mappings.
    pub(crate) fn has_external(&self) -> bool {
        !self.external.is_empty()
    }

    /// Remap only one own source through an admitted host map. New source
    /// rows have already been appended to `sources`, in the host's order.
    /// Other sources keep their original byte correspondence and wire tokens.
    /// No text is written and no JSON is encoded.
    pub(crate) fn remap_source(
        &mut self,
        code: &str,
        source: u32,
        host: &DecodedFragmentMap,
        sources: &[MapSource<'_>],
    ) -> Result<(), crate::assembly::ArtifactSchemaError> {
        use crate::assembly::map_input::SourcePayload;
        use crate::assembly::svelte_module::{generated_line_starts, utf16_offset_to_bytes};
        use crate::assembly::ArtifactSchemaError as E;
        if self.has_external() || source >= self.own_sources {
            return Err(E::UnqualifiedMap);
        }
        let base = self.own_sources;
        let count = u32::try_from(host.sources().len()).map_err(|_| E::InvalidIdentity)?;
        let total = base.checked_add(count).ok_or(E::InvalidIdentity)?;
        if sources.len() != total as usize {
            return Err(E::UnqualifiedMap);
        }
        // Reuse the admitted map's existing token lookup. This is a typed
        // source-map view, never an intermediate JSON encoding.
        let host_map = map_compose::to_source_map(host);
        let lookup = host_map.generate_lookup_table();
        let name_base = self.host_names.len() as u32;
        let remap = |line, column| {
            #[cfg(test)]
            HOST_REMAP_LOOKUPS.with(|count| count.set(count.get() + 1));
            host_map
                .lookup_token(&lookup, line, column)
                .and_then(|token| {
                    token.get_source_id().map(|index| SourcePayload {
                        source_index: base + index,
                        source_line: token.get_src_line(),
                        source_column: token.get_src_col(),
                        name_index: token.get_name_id().map(|index| name_base + index),
                    })
                })
        };
        let wire: Vec<_> = self
            .wire
            .iter()
            .map(|token| WireSegment {
                payload: match token.payload {
                    Some(payload) if payload.source_index == source => {
                        remap(payload.source_line, payload.source_column)
                    }
                    payload => payload,
                },
                ..*token
            })
            .collect();
        let generated_lines = generated_line_starts(code);
        let offsets: Vec<_> = wire
            .iter()
            .map(|token| {
                utf16_offset_to_bytes(
                    code,
                    &generated_lines,
                    token.generated_line,
                    token.generated_column,
                )
                .ok_or(E::InvalidGeneratedRange)
            })
            .collect::<Result<_, _>>()?;
        let source_lines: Vec<_> = sources
            .iter()
            .map(|source| source.content.map(generated_line_starts))
            .collect();
        let mut segments = Vec::with_capacity(self.segments.len());
        for segment in &self.segments {
            if segment.source != source {
                segments.push(segment.clone());
                continue;
            }
            let mut start = segment.generated.start;
            let first = offsets.partition_point(|offset| *offset < start);
            let end = offsets.partition_point(|offset| *offset < segment.generated.end);
            if first == end || offsets[first] != start {
                return Err(E::UnqualifiedMap);
            }
            let mut payload = wire[first].payload;
            for index in first + 1..end {
                append_remapped_segment(
                    &mut segments,
                    start..offsets[index],
                    payload,
                    sources,
                    &source_lines,
                )?;
                start = offsets[index];
                payload = wire[index].payload;
            }
            append_remapped_segment(
                &mut segments,
                start..segment.generated.end,
                payload,
                sources,
                &source_lines,
            )?;
        }
        self.segments = segments;
        self.wire = wire;
        self.own_sources = total;
        self.host_names.extend(host.names().iter().cloned());
        self.host_ignore_list
            .extend(host.ignore_list().iter().map(|index| base + *index as u32));
        Ok(())
    }

    /// The position the encoded map answers for `generated`: the index of
    /// one of the artifact's own sources and a byte offset in it. A token
    /// applies until the next one, so a rewritten or anchored run answers
    /// its source point at every byte, and an identity run the byte at the
    /// same distance. Bytes of an external piece are answered by its own
    /// admitted map, which only the encoded map carries.
    pub fn lookup(&self, generated: u32) -> Option<(u32, u32)> {
        let index = self
            .segments
            .partition_point(|segment| segment.generated.end <= generated);
        let segment = self.segments.get(index)?;
        if segment.generated.start > generated {
            return None;
        }
        let offset = match segment.kind {
            MapSegmentKind::Identity => {
                segment.source_start + (generated - segment.generated.start)
            }
            MapSegmentKind::Rewritten | MapSegmentKind::Anchor => segment.source_start,
        };
        Some((segment.source, offset))
    }

    /// Encode as V3 over `code`, the artifact's text. `sources` names the
    /// artifact's own sources in the order of [`EmittedArtifact::sources`];
    /// external maps bring their own rows. Builds a
    /// [`DecodedFragmentMap`] and encodes it through
    /// [`map_compose::to_source_map`]; there is no other encoder.
    pub fn to_source_map(
        &self,
        code: &str,
        sources: &[MapSource<'_>],
        source_root: Option<&str>,
    ) -> oxc_sourcemap::SourceMap<'static> {
        verter_debug_assert_eq!(sources.len(), self.own_sources as usize);
        let own = (0..self.own_sources as usize).map(|index| sources.get(index).copied());
        let mut names = self.host_names.clone();
        let mut rows: Vec<String> = Vec::new();
        let mut contents: Vec<Option<String>> = Vec::new();
        let mut ignore_list = self
            .host_ignore_list
            .iter()
            .map(|&index| f64::from(index))
            .collect::<Vec<_>>();
        for source in own {
            rows.push(source.map_or_else(String::new, |source| source.name.to_owned()));
            contents.push(source.and_then(|source| source.content).map(str::to_owned));
        }
        for tables in &self.external {
            rows.extend(tables.sources.iter().cloned());
            contents.extend(tables.sources_content.iter().cloned());
            names.extend(tables.names.iter().cloned());
            ignore_list.extend(tables.ignore_list.iter().map(|&entry| f64::from(entry)));
        }
        let contents = contents.iter().any(Option::is_some).then_some(contents);
        let map = DecodedFragmentMap::from_segments(code, rows, contents, names, self.wire.clone())
            .with_layout(source_root.map(str::to_owned), ignore_list);
        map_compose::to_source_map(&map)
    }
}

fn append_remapped_segment(
    segments: &mut Vec<DecodedMapSegment>,
    generated: std::ops::Range<u32>,
    payload: Option<crate::assembly::map_input::SourcePayload>,
    sources: &[MapSource<'_>],
    lines: &[Option<Vec<u32>>],
) -> Result<(), crate::assembly::ArtifactSchemaError> {
    use crate::assembly::ArtifactSchemaError as E;
    let Some(payload) = payload.filter(|_| !generated.is_empty()) else {
        return Ok(());
    };
    let index = payload.source_index as usize;
    let text = sources
        .get(index)
        .and_then(|source| source.content)
        .ok_or(E::MapSourceWithoutContent)?;
    let lines = lines
        .get(index)
        .and_then(Option::as_deref)
        .ok_or(E::MapSourceWithoutContent)?;
    let offset = crate::assembly::svelte_module::utf16_offset_to_bytes(
        text,
        lines,
        payload.source_line,
        payload.source_column,
    )
    .ok_or(E::InvalidSourceRange)?;
    segments.push(DecodedMapSegment {
        generated,
        source: payload.source_index,
        source_start: offset,
        kind: MapSegmentKind::Anchor,
    });
    Ok(())
}

/// Final text of one artifact. Fields are private; the text is read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedArtifact {
    text: String,
    map: Option<DecodedMap>,
    boundaries: Vec<(ArtifactBoundary, u32)>,
    sources: Vec<SourceKey>,
    grammar: Option<GrammarWitness>,
}

impl EmittedArtifact {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// `None` exactly when the plan was `MapMode::NoMap`.
    pub fn map(&self) -> Option<&DecodedMap> {
        self.map.as_ref()
    }

    /// Output offset of each boundary, in plan order.
    pub fn boundaries(&self) -> &[(ArtifactBoundary, u32)] {
        &self.boundaries
    }

    /// The artifact's own sources, in source-table order.
    pub fn sources(&self) -> &[SourceKey] {
        &self.sources
    }

    /// The grammar the whole artifact was witnessed in by its producers,
    /// when it was; such text needs no parse before publication.
    pub fn grammar(&self) -> Option<GrammarWitness> {
        self.grammar
    }

    /// The text and map, for an assembly step that publishes them.
    pub(crate) fn into_parts(self) -> (String, Option<DecodedMap>) {
        (self.text, self.map)
    }
}

pub(crate) type EmittedWithMarkers = (
    EmittedArtifact,
    Vec<(crate::emission::MarkerId, std::ops::Range<u32>)>,
);

impl<'s> EmitPlan<'s> {
    /// Finalise: one buffer of the exact length, one generated-order pass.
    /// The text-only and the mapped write are separate specialisations,
    /// chosen once by the plan's `MapMode`.
    pub fn write<M: ObserveExec>(
        self,
        ledger: &mut CompilerWorkLedger<M>,
    ) -> Result<EmittedArtifact, EmitPlanError> {
        self.write_with_markers(ledger, false)
            .map(|(artifact, _)| artifact)
    }

    /// Write once, also retaining the final ranges of surviving producer
    /// markers. Replaced markers disappear; markers in a fill are reported
    /// where that fill was written.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn write_marked<M: ObserveExec>(
        self,
        ledger: &mut CompilerWorkLedger<M>,
    ) -> Result<EmittedWithMarkers, EmitPlanError> {
        self.write_with_markers(ledger, true)
    }

    fn write_with_markers<M: ObserveExec>(
        self,
        ledger: &mut CompilerWorkLedger<M>,
        capture_markers: bool,
    ) -> Result<EmittedWithMarkers, EmitPlanError> {
        let mut text = String::with_capacity(self.len as usize);
        ledger.record(WorkKind::EmissionAllocations, 1);
        let mut boundaries = Vec::new();
        let markers;
        let map = match self.mode {
            MapMode::NoMap => {
                let mut writer = Writer::new(&mut text, ledger, TextOnly);
                if capture_markers {
                    writer.capture_markers();
                }
                write_plan(&self, &mut writer, &mut boundaries);
                markers = writer.take_markers();
                None
            }
            MapMode::Map => {
                let mut writer = Writer::new(&mut text, ledger, MapSink::new(&self.sources));
                if capture_markers {
                    writer.capture_markers();
                }
                write_plan(&self, &mut writer, &mut boundaries);
                markers = writer.take_markers();
                let MapParts {
                    segments,
                    wire,
                    external,
                } = writer.into_sink().finish();
                ledger.record(WorkKind::EmissionMapSegments, segments.len() as u64);
                Some(DecodedMap {
                    segments,
                    wire,
                    own_sources: self.sources.len() as u32,
                    external,
                    host_names: Vec::new(),
                    host_ignore_list: Vec::new(),
                })
            }
        };
        verter_debug_assert_eq!(text.len(), self.len as usize);
        Ok((
            EmittedArtifact {
                text,
                map,
                boundaries,
                grammar: self.grammar,
                sources: self.sources.into_iter().map(|source| source.key).collect(),
            },
            markers,
        ))
    }
}

fn write_plan<'s, S: EmissionSink<'s>, M: ObserveExec>(
    plan: &EmitPlan<'s>,
    writer: &mut Writer<'_, '_, S, M>,
    boundaries: &mut Vec<(ArtifactBoundary, u32)>,
) {
    for segment in &plan.segments {
        writer.ledger().record(WorkKind::EmissionSegments, 1);
        match *segment {
            EmissionSegment::Fragment(fragment) => {
                let fills = fills_of(plan, FillHost::Placed(fragment.0));
                writer.write_fragment(&plan.fragments[fragment.0 as usize], &fills);
            }
            EmissionSegment::Slot(slot) => match &plan.slots[slot.0 as usize] {
                SlotBody::Fragment(fragment) => {
                    let fills = fills_of(plan, FillHost::Slot(slot.0));
                    writer.write_fragment(fragment, &fills);
                }
                SlotBody::Plan(nested) => write_plan(nested, writer, boundaries),
                SlotBody::Empty => {}
                SlotBody::Reserved => {
                    verter_debug_assert!(false, "a finished plan has no unfilled slot");
                }
            },
            EmissionSegment::Boundary(boundary) => boundaries.push((boundary, writer.written())),
        }
    }
}

/// The fills of the fragment at `host`, in piece order.
fn fills_of<'p, 's>(plan: &'p EmitPlan<'s>, host: FillHost) -> Vec<Fill<'p, 's>> {
    let first = plan.marked.partition_point(|fill| fill.host < host);
    plan.marked[first..]
        .iter()
        .take_while(|fill| fill.host == host)
        .map(|fill| Fill {
            piece: fill.piece,
            with: &fill.with,
        })
        .collect()
}

#[cfg(test)]
#[path = "artifact_tests.rs"]
mod tests;
