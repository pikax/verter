//! Repeated reads are occurrences in authored pieces, before emission.
use super::effect::EffectGroupId;
use super::effect::VaporEffect;
use super::graph::{EffectRelations, VaporEffectNode};
use super::ops::{Part, RunBuf, Runs};
use crate::framework::vue::target::VueTargetRefusal;
use oxc_allocator::Allocator;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_span::Span;

#[derive(Debug)]
pub(super) struct ReadOccurrence {
    pub(super) prefix: usize,
    pub(super) last: usize,
    pub(super) sources: Vec<Span>,
    pub(super) token: String,
    pub(super) read: Option<EffectGroupId>,
}

pub(super) struct RecordedEffect<'c> {
    pub(super) parts: RunBuf<'c>,
    pub(super) reads: Vec<ReadOccurrence>,
}

pub(super) fn record<'c>(
    effect: &VaporEffect<'c>,
    runs: &Runs<'c>,
    source: &'c str,
) -> RecordedEffect<'c> {
    let mut initial = Vec::new();
    super::output::record_effect(effect, &mut initial);
    let mut pending: Vec<_> = initial.into_iter().rev().collect();
    let mut parts = Vec::new();
    while let Some(part) = pending.pop() {
        if let Part::Run(run) = part {
            pending.extend(runs.get(run).iter().rev().copied());
        } else {
            parts.push(part);
        }
    }
    let mut reads = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    for (index, part) in parts.iter().enumerate() {
        if let Part::Source(span) = part {
            if quote.is_none() && index > 0 && parts[index - 1] == Part::Text("_ctx.") {
                if let Some((last, sources, token)) = read_token(&parts, index, source) {
                    reads.push(ReadOccurrence {
                        prefix: index - 1,
                        last,
                        sources,
                        token,
                        read: None,
                    });
                }
            }
            for byte in source[span.start as usize..span.end as usize].bytes() {
                if quote.is_some() {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if Some(byte) == quote {
                        quote = None;
                    }
                } else if matches!(byte, b'\'' | b'"' | b'`') {
                    quote = Some(byte);
                }
            }
        }
    }
    RecordedEffect { parts, reads }
}

#[derive(Clone)]
struct SourceCursor<'a, 'c> {
    parts: &'a [Part<'c>],
    source: &'c str,
    part: usize,
    offset: u32,
}
impl SourceCursor<'_, '_> {
    fn peek(&mut self) -> Option<u8> {
        loop {
            let Part::Source(span) = self.parts.get(self.part)? else {
                return None;
            };
            if self.offset < span.end {
                return self.source.as_bytes().get(self.offset as usize).copied();
            }
            self.part += 1;
            let Part::Source(next) = self.parts.get(self.part)? else {
                return None;
            };
            self.offset = next.start;
        }
    }
    fn take_ident(&mut self, token: &mut String) -> Option<(usize, u32)> {
        let mut end = None;
        while self.peek().is_some_and(ident_continue) {
            let byte = self.peek()?;
            token.push(byte as char);
            end = Some((self.part, self.offset + 1));
            self.offset += 1;
        }
        end
    }
}
fn read_token(
    parts: &[Part<'_>],
    index: usize,
    source: &str,
) -> Option<(usize, Vec<Span>, String)> {
    let Part::Source(first) = parts[index] else {
        return None;
    };
    let mut cursor = SourceCursor {
        parts,
        source,
        part: index,
        offset: first.start,
    };
    if !cursor.peek().is_some_and(ident_start) {
        return None;
    }
    let mut token = String::new();
    let mut end = cursor.take_ident(&mut token)?;
    while cursor.peek() == Some(b'.') {
        let mut next = cursor.clone();
        next.offset += 1;
        if !next.peek().is_some_and(ident_start) {
            break;
        }
        token.push('.');
        cursor = next;
        end = cursor.take_ident(&mut token)?;
    }
    if cursor.peek() == Some(b'(') {
        return None;
    }
    let mut sources = Vec::new();
    for (part_index, part) in parts.iter().enumerate().take(end.0 + 1).skip(index) {
        let Part::Source(span) = part else {
            return None;
        };
        sources.push(Span::new(
            span.start,
            if part_index == end.0 { end.1 } else { span.end },
        ));
    }
    Some((end.0, sources, token))
}

fn ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')
}
fn ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

pub(super) struct Hoisted<'c> {
    pub(super) declarations: RunBuf<'c>,
    pub(super) names: FxHashMap<EffectGroupId, &'c str>,
}

pub(super) fn hoist<'c>(
    graph: &EffectRelations<'c>,
    records: &[Option<RecordedEffect<'c>>],
    group: EffectGroupId,
    tokens: &[String],
    allocator: &'c Allocator,
    indent: &'c str,
) -> Result<Hoisted<'c>, VueTargetRefusal> {
    let mut counts: FxHashMap<EffectGroupId, (usize, Vec<Span>)> = FxHashMap::default();
    let mut order = Vec::new();
    for (effect, _) in graph.edges(group) {
        let row = super::graph::effect_row(graph, *effect)?;
        let record = records[row.0 as usize]
            .as_ref()
            .expect("effect occurrence record");
        let mut local: FxHashMap<EffectGroupId, (usize, Vec<Span>)> = FxHashMap::default();
        for occurrence in &record.reads {
            let read = occurrence.read.expect("registered read");
            let count = local
                .entry(read)
                .or_insert_with(|| (0, occurrence.sources.clone()));
            count.0 += 1;
        }
        for (read, _) in graph.edges(*effect) {
            let (weight, first) = local.get(read).expect("read dependency has an occurrence");
            let count = counts.entry(*read).or_insert_with(|| {
                order.push(*read);
                (0, first.clone())
            });
            count.0 += weight;
        }
    }
    let mut used = FxHashSet::default();
    let mut names = FxHashMap::default();
    let mut declarations = Vec::new();
    for read in order {
        let (count, spans) = &counts[&read];
        if *count < 2 {
            continue;
        }
        let Some(VaporEffectNode::Read { token }) = graph.node(read) else {
            unreachable!("registered read node")
        };
        let name = allocator.alloc_str(&gen_var_name(&tokens[*token as usize], &mut used));
        names.insert(read, name);
        declarations.extend([
            Part::Text(indent),
            Part::Text("  const "),
            Part::Text(name),
            Part::Text(" = _ctx."),
        ]);
        declarations.extend(spans.iter().copied().map(Part::Source));
        declarations.push(Part::Text("\n"));
    }
    Ok(Hoisted {
        declarations,
        names,
    })
}

pub(super) fn substitute<'c>(
    record: &RecordedEffect<'c>,
    names: &FxHashMap<EffectGroupId, &'c str>,
    out: &mut RunBuf<'c>,
) {
    let mut index = 0;
    for occurrence in &record.reads {
        let Some(name) = names.get(&occurrence.read.expect("registered read")) else {
            continue;
        };
        out.extend_from_slice(&record.parts[index..occurrence.prefix]);
        out.push(Part::Text(name));
        if let Part::Source(last) = record.parts[occurrence.last] {
            if last.end > occurrence.sources.last().expect("read span").end {
                out.push(Part::Source(Span::new(
                    occurrence.sources.last().expect("read span").end,
                    last.end,
                )));
            }
        }
        index = occurrence.last + 1;
    }
    out.extend_from_slice(&record.parts[index..]);
}

fn gen_var_name(token: &str, used: &mut FxHashSet<String>) -> String {
    let raw = token.strip_prefix("_ctx.").unwrap_or(token);
    let mut cleaned = String::with_capacity(raw.len());
    let mut last_was_sep = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            cleaned.push(c);
            last_was_sep = false;
        } else if !last_was_sep {
            cleaned.push('_');
            last_was_sep = true;
        }
    }
    while cleaned.ends_with('_') {
        cleaned.pop();
    }
    let base = format!("_{cleaned}");
    if used.insert(base.clone()) {
        return base;
    }
    let mut i = 1u32;
    loop {
        let candidate = format!("{base}_{i}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        i += 1;
    }
}
