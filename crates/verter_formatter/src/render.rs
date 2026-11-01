//! Deterministic two-pass renderer. Flat widths are composed once in postorder;
//! each selected node is then emitted once, with no speculative subtree search.
use crate::coordinates::FormattedByteRange;
use crate::doc::{Doc, DocId, Line, Node};
use crate::provenance::Provenanced;
use crate::width::{columns, strip_ansi};

#[derive(Debug, Clone, Copy)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}
impl LineEnding {
    fn text(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// Already-normalized layout options; source/config discovery belongs upstream.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub print_width: usize,
    pub tab_width: usize,
    pub use_tabs: bool,
    pub line_ending: LineEnding,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            print_width: 80,
            tab_width: 2,
            use_tabs: false,
            line_ending: LineEnding::Lf,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderedSegment {
    pub range: FormattedByteRange,
}

/// Generated whitespace/text has no invented authored binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Source(Provenanced<RenderedSegment>),
    Generated(RenderedSegment),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderWork {
    /// Postorder node visits, emitted node visits and group decisions.
    pub doc_visits: usize,
    pub peak_stack_entries: usize,
    pub emitted_bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RenderedDoc {
    pub text: String,
    pub segments: Vec<Segment>,
    pub work: RenderWork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    ForeignRoot,
    InvalidTabWidth,
    OutputTooLarge,
}
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RenderError {}

// A tabbed run is f(c) = floor((c + prefix) / tab) * tab + tail.
// Without a tab it is f(c) = c + prefix. Composition takes constant work.
#[derive(Clone, Copy, Default)]
struct Width {
    prefix: usize,
    tail: Option<usize>,
    hard: bool,
}
impl Width {
    fn apply(self, column: usize, tab: usize) -> usize {
        let column = column.saturating_add(self.prefix);
        match self.tail {
            Some(tail) => (column / tab).saturating_mul(tab).saturating_add(tail),
            None => column,
        }
    }
    fn then(self, next: Self, tab: usize) -> Self {
        let (prefix, tail) = match self.tail {
            Some(tail) => (self.prefix, Some(next.apply(tail, tab))),
            None => (self.prefix.saturating_add(next.prefix), next.tail),
        };
        Self {
            prefix,
            tail,
            hard: self.hard || next.hard,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Summary {
    body: Width,
    suffix: Width,
}
impl Summary {
    fn then(self, next: Self, tab: usize) -> Self {
        Self {
            body: self.body.then(next.body, tab),
            suffix: self.suffix.then(next.suffix, tab),
        }
    }
}

// Tabs advance to the next stop after ANSI stripping, not one fixed width.
fn width(text: &str, tab: usize) -> Width {
    let visible = strip_ansi(text);
    let hard = text.contains(['\r', '\n']);
    let last_line = visible.rsplit(['\r', '\n']).next().unwrap_or("");
    let mut result = Width::default();
    for (index, part) in last_line.split('\t').enumerate() {
        if index != 0 {
            result = result.then(
                Width {
                    prefix: 0,
                    tail: Some(tab),
                    hard: false,
                },
                tab,
            );
        }
        result = result.then(
            Width {
                prefix: columns(part),
                tail: None,
                hard: part.contains(['\n', '\r']),
            },
            tab,
        );
    }
    result.hard = hard;
    result
}

fn summaries(doc: &Doc, tab: usize, work: &mut RenderWork) -> Vec<Summary> {
    let mut widths: Vec<Summary> = Vec::with_capacity(doc.len());
    for node in &doc.nodes {
        work.doc_visits += 1;
        let summary = match node {
            Node::Text(text, _) => Summary {
                body: width(text, tab),
                ..Default::default()
            },
            Node::Suffix(text, _) => Summary {
                suffix: width(text, tab),
                ..Default::default()
            },
            Node::Concat(children) => children
                .iter()
                .fold(Summary::default(), |sum, &id| sum.then(widths[id], tab)),
            Node::Group(body) | Node::Indent(_, body) | Node::Align(_, body) => widths[*body],
            Node::IfBreak { flat, .. } => widths[*flat],
            Node::Line(line) => Summary {
                body: Width {
                    prefix: usize::from(matches!(line, Line::Space)),
                    hard: matches!(line, Line::Hard),
                    tail: None,
                },
                ..Default::default()
            },
        };
        widths.push(summary);
    }
    widths
}

#[derive(Clone, Copy)]
struct Command {
    id: usize,
    indent: usize,
    flat: bool,
}

struct Sink {
    output: RenderedDoc,
    column: usize,
    pending_indent: Option<usize>,
    options: RenderOptions,
    summaries: Vec<Summary>,
}
impl Sink {
    fn append(
        &mut self,
        text: &str,
        binding: Option<crate::provenance::AuthoredProvenance>,
        measured: Option<Width>,
    ) -> Result<(), RenderError> {
        if text.is_empty() {
            return Ok(());
        }
        if let Some(indent) = self.pending_indent.take() {
            if !text.starts_with(['\r', '\n']) {
                let tabs = if self.options.use_tabs {
                    indent / self.options.tab_width
                } else {
                    0
                };
                let spaces = indent - tabs * self.options.tab_width;
                if indent > u32::MAX as usize {
                    return Err(RenderError::OutputTooLarge);
                }
                let padding = "\t".repeat(tabs) + &" ".repeat(spaces);
                self.append(&padding, None, None)?;
            }
        }
        let start = self.output.text.len();
        let end = start
            .checked_add(text.len())
            .filter(|&n| n <= u32::MAX as usize)
            .ok_or(RenderError::OutputTooLarge)?;
        self.output.text.push_str(text);
        self.output.work.emitted_bytes += text.len();
        let segment = RenderedSegment {
            range: FormattedByteRange::for_emitted(&self.output.text, start as u32, end as u32)
                .expect("ordered output offsets"),
        };
        self.output.segments.push(match binding {
            Some(binding) => Segment::Source(Provenanced::new(segment, binding)),
            None => Segment::Generated(segment),
        });
        let measure = measured.unwrap_or_else(|| width(text, self.options.tab_width));
        if measure.hard {
            self.column = 0;
        }
        self.column = measure.apply(self.column, self.options.tab_width);
        Ok(())
    }
    fn column(&self) -> usize {
        self.pending_indent.unwrap_or(self.column)
    }
}

/// Render a root in its owning arena. Top-level lines break; groups flatten
/// only when their body and all pending suffixes fit. Nested groups inherit a
/// flat parent, otherwise decide independently. Raw text is never normalized.
/// No final newline is invented: syntax printers express it as a Hard line.
pub fn render(doc: &Doc, root: DocId, options: RenderOptions) -> Result<RenderedDoc, RenderError> {
    if !doc.contains(root) {
        return Err(RenderError::ForeignRoot);
    }
    if options.tab_width == 0 {
        return Err(RenderError::InvalidTabWidth);
    }
    let mut work = RenderWork::default();
    let summaries = summaries(doc, options.tab_width, &mut work);
    let mut sink = Sink {
        output: RenderedDoc {
            text: String::new(),
            segments: Vec::new(),
            work,
        },
        column: 0,
        pending_indent: None,
        options,
        summaries,
    };
    let mut stack = vec![Command {
        id: root.index,
        indent: 0,
        flat: false,
    }];
    let mut suffixes = Vec::new();
    let mut pending_width = Width::default();
    while let Some(cmd) = stack.pop() {
        sink.output.work.peak_stack_entries = sink
            .output
            .work
            .peak_stack_entries
            .max(stack.len() + suffixes.len() + 1);
        sink.output.work.doc_visits += 1;
        let push =
            |stack: &mut Vec<Command>, id, indent, flat| stack.push(Command { id, indent, flat });
        match &doc.nodes[cmd.id] {
            Node::Text(text, binding) => {
                // Flush suffixes before the first raw newline as well. Split at
                // that boundary only; the remaining literal stays one segment.
                if let Some(at) = text.find(['\r', '\n']).filter(|_| !suffixes.is_empty()) {
                    sink.append(&text[..at], *binding, None)?;
                    flush(doc, &mut suffixes, &mut sink)?;
                    pending_width = Width::default();
                    sink.append(&text[at..], *binding, Some(sink.summaries[cmd.id].body))?;
                } else {
                    sink.append(text, *binding, Some(sink.summaries[cmd.id].body))?;
                }
            }
            Node::Concat(children) => {
                for &id in children.iter().rev() {
                    push(&mut stack, id, cmd.indent, cmd.flat);
                }
            }
            Node::Group(body) => {
                sink.output.work.doc_visits += 1;
                let summary = sink.summaries[*body];
                let end = summary.suffix.apply(
                    pending_width.apply(
                        summary.body.apply(sink.column(), options.tab_width),
                        options.tab_width,
                    ),
                    options.tab_width,
                );
                let flat = !summary.body.hard && (cmd.flat || end <= options.print_width);
                push(&mut stack, *body, cmd.indent, flat);
            }
            Node::Indent(columns, body) => push(
                &mut stack,
                *body,
                cmd.indent.saturating_add(*columns),
                cmd.flat,
            ),
            Node::Align(columns, body) => push(
                &mut stack,
                *body,
                sink.column().saturating_add(*columns),
                cmd.flat,
            ),
            Node::IfBreak { broken, flat } => push(
                &mut stack,
                if cmd.flat { *flat } else { *broken },
                cmd.indent,
                cmd.flat,
            ),
            Node::Line(line) if cmd.flat && !matches!(line, Line::Hard) => {
                if matches!(line, Line::Space) {
                    sink.append(" ", None, None)?;
                }
            }
            Node::Line(_) => {
                flush(doc, &mut suffixes, &mut sink)?;
                pending_width = Width::default();
                sink.append(options.line_ending.text(), None, None)?;
                sink.pending_indent = Some(cmd.indent);
            }
            Node::Suffix(_, _) => {
                suffixes.push(cmd.id);
                pending_width =
                    pending_width.then(sink.summaries[cmd.id].suffix, options.tab_width);
            }
        }
    }
    flush(doc, &mut suffixes, &mut sink)?;
    Ok(sink.output)
}

fn flush(doc: &Doc, suffixes: &mut Vec<usize>, sink: &mut Sink) -> Result<(), RenderError> {
    for id in suffixes.drain(..) {
        if let Node::Suffix(text, binding) = &doc.nodes[id] {
            sink.output.work.doc_visits += 1;
            sink.append(text, *binding, Some(sink.summaries[id].suffix))?;
        }
    }
    Ok(())
}
