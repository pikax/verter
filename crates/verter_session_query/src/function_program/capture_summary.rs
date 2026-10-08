//! The per-file lexical-capture summary: every served function position's
//! TRANSITIVE closure captures and captured reads, frozen once when
//! [`FunctionProgramIndex::from_discovery`](super::FunctionProgramIndex::from_discovery)
//! seals the file and shared by every entry.
//!
//! A function captures what its own body and every function nested in it
//! reference from a frame enclosing it. Stored per function, those sets
//! repeat each nested function's captures in every enclosing function: a
//! chain of nested functions reading the variables of the frames around
//! it costs the square of its depth, and each enclosing function copies
//! its children's records again.
//!
//! The summary stores each capture and each captured read ONCE, where it
//! is authored, in one source-ordered arena per file in which every
//! function's own occurrences and its nested functions' occurrences form
//! one contiguous range. A function's transitive set is its range,
//! filtered to the occurrences whose binding a frame ENCLOSING the
//! function declares (the occurrence records the depth of that frame) and
//! to the FIRST occurrence of each binding within the range (the
//! occurrence records the previous occurrence of the same binding). The
//! filters read only the occurrence itself, so no function's set is ever
//! materialized and no enclosing function copies a nested one's.
//!
//! The order is the order the per-function sets always had: a function's
//! own occurrences in source order, each nested function's set placed at
//! the nested function's start.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{
    FlowBindingIdentity, FunctionCapturedRead, FunctionProgramDiscovery, FunctionProgramKey,
    FunctionReferenceBinding, FunctionWriteTarget,
};

/// No earlier occurrence of the same binding in the arena.
const FIRST: u32 = u32::MAX;

/// One capture occurrence: a reference or write of a binding an enclosing
/// frame declares.
#[derive(Debug)]
struct CaptureOccurrence {
    /// The binding, by its position in [`CaptureSummaries::bindings`].
    binding: u32,
    /// One past the depth of the frame declaring the binding: the
    /// occurrence belongs to the set of each function at least this deep
    /// whose range holds it. `0` when no enclosing frame declares it.
    visible_from: u32,
    /// The previous occurrence of the same binding, or [`FIRST`].
    previous: u32,
}

/// One captured read: a read of a binding an enclosing frame declares,
/// with its static path.
#[derive(Debug)]
struct ReadOccurrence {
    read: FunctionCapturedRead,
    /// As [`CaptureOccurrence::visible_from`].
    visible_from: u32,
    /// The previous read of the same binding along the same path, or
    /// [`FIRST`].
    previous: u32,
}

/// One function position's place in the arenas.
#[derive(Debug)]
struct FrameSummary {
    key: FunctionProgramKey,
    span: verter_span::Span,
    /// Nesting depth below the outermost enclosing function.
    depth: u32,
    captures: (u32, u32),
    reads: (u32, u32),
    /// The functions nested directly in this one, in
    /// [`CaptureSummaries::children`].
    children: (u32, u32),
    /// Whether this function itself creates no callable no entry serves.
    own_exhaustive: bool,
    /// Whether neither this function nor any function nested in it
    /// creates one.
    exhaustive: bool,
}

/// The frozen capture summary of one file. Owned by the file's
/// [`FunctionProgramIndex`](super::FunctionProgramIndex) and shared by its
/// entries; released with the last of them.
#[derive(Debug, Default)]
pub(super) struct CaptureSummaries {
    /// By entry ordinal.
    frames: Box<[FrameSummary]>,
    children: Box<[u32]>,
    /// Every captured binding once, in first-occurrence order.
    bindings: Box<[FlowBindingIdentity]>,
    captures: Box<[CaptureOccurrence]>,
    reads: Box<[ReadOccurrence]>,
}

/// One frame's own occurrences and directly nested functions, in order.
enum Item<'d> {
    Capture(&'d FlowBindingIdentity),
    Read(&'d FunctionCapturedRead),
    Child(u32),
}

/// A frame whose range is being filled.
struct Open<'d> {
    frame: u32,
    items: std::vec::IntoIter<Item<'d>>,
    /// The depth the frame's key had on the chain before the frame
    /// entered it.
    shadowed: Option<u32>,
    captures_start: u32,
    reads_start: u32,
    children: (u32, u32),
}

/// What [`CaptureSummaries::freeze`] reads.
struct FreezeInputs<'d> {
    entries: &'d [FunctionProgramDiscovery],
    own_reads: &'d [Vec<FunctionCapturedRead>],
    children_of: &'d [Vec<u32>],
}

impl<'d> FreezeInputs<'d> {
    /// Open `frame` at `depth`: enter its key on the chain and order its
    /// own occurrences with its nested functions. A frame's own
    /// occurrence precedes a nested function starting at the same offset;
    /// otherwise the order is stable.
    fn open(
        &self,
        frame: u32,
        depth: u32,
        chain: &mut FxHashMap<&'d FunctionProgramKey, u32>,
        children: &mut Vec<u32>,
        captures_start: u32,
        reads_start: u32,
    ) -> Open<'d> {
        let entry = &self.entries[frame as usize];
        let shadowed = chain.insert(&entry.key, depth);
        let mut items: Vec<(u32, bool, Item<'d>)> = Vec::new();
        for reference in entry.references.iter() {
            if let FunctionReferenceBinding::Resolved(binding) = &reference.binding {
                if binding.defining_function != entry.key {
                    items.push((reference.span.start, false, Item::Capture(binding)));
                }
            }
        }
        for target in entry.writes.iter().flat_map(|write| write.targets.iter()) {
            if let FunctionWriteTarget::Binding { reference, .. } = target {
                if let FunctionReferenceBinding::Resolved(binding) = &reference.binding {
                    if binding.defining_function != entry.key {
                        items.push((reference.span.start, false, Item::Capture(binding)));
                    }
                }
            }
        }
        for read in self.own_reads[frame as usize].iter() {
            items.push((read.span.start, false, Item::Read(read)));
        }
        let first_child = children.len() as u32;
        for &child in &self.children_of[frame as usize] {
            children.push(child);
            items.push((
                self.entries[child as usize].span.start,
                true,
                Item::Child(child),
            ));
        }
        items.sort_by_key(|(start, nested, _)| (*start, *nested));
        Open {
            frame,
            items: items
                .into_iter()
                .map(|(_, _, item)| item)
                .collect::<Vec<_>>()
                .into_iter(),
            shadowed,
            captures_start,
            reads_start,
            children: (first_child, children.len() as u32),
        }
    }
}

impl CaptureSummaries {
    /// Freeze the transitive captures of every entry of one file from its
    /// resolved references and writes and its lexical parent links.
    pub(super) fn freeze(entries: &[FunctionProgramDiscovery]) -> Self {
        let mut position_of: FxHashMap<&FunctionProgramKey, u32> =
            FxHashMap::with_capacity_and_hasher(entries.len(), Default::default());
        for (position, entry) in entries.iter().enumerate() {
            position_of.entry(&entry.key).or_insert(position as u32);
        }
        let mut children_of: Vec<Vec<u32>> = vec![Vec::new(); entries.len()];
        let mut roots = Vec::new();
        for (position, entry) in entries.iter().enumerate() {
            match entry
                .lexical_parent
                .as_deref()
                .and_then(|parent| position_of.get(parent))
            {
                Some(&parent) if parent as usize != position => {
                    children_of[parent as usize].push(position as u32);
                }
                _ => roots.push(position as u32),
            }
        }
        // Each frame's own captured reads: a read records its own span and
        // path, so it is kept apart from the frame's capture occurrences.
        let own_reads: Vec<Vec<FunctionCapturedRead>> = entries
            .iter()
            .map(|entry| {
                entry
                    .references
                    .iter()
                    .filter(|reference| reference.read_role.is_some())
                    .filter_map(|reference| match &reference.binding {
                        FunctionReferenceBinding::Resolved(binding)
                            if binding.defining_function != entry.key =>
                        {
                            Some(FunctionCapturedRead {
                                binding: binding.clone(),
                                path: Arc::clone(&reference.path),
                                span: reference.span,
                            })
                        }
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        let inputs = FreezeInputs {
            entries,
            own_reads: &own_reads,
            children_of: &children_of,
        };

        let mut frames: Vec<Option<FrameSummary>> = entries.iter().map(|_| None).collect();
        let mut children: Vec<u32> = Vec::with_capacity(entries.len());
        let mut bindings: Vec<FlowBindingIdentity> = Vec::new();
        let mut binding_ids: FxHashMap<&FlowBindingIdentity, u32> = FxHashMap::default();
        let mut last_capture: Vec<u32> = Vec::new();
        let mut last_read: FxHashMap<(u32, &[Arc<str>]), u32> = FxHashMap::default();
        let mut captures: Vec<CaptureOccurrence> = Vec::new();
        let mut reads: Vec<ReadOccurrence> = Vec::new();
        // The depth of the innermost open frame of each key.
        let mut chain: FxHashMap<&FunctionProgramKey, u32> = FxHashMap::default();
        // The open frames, outermost first: a frame's depth is its place
        // here, and nesting costs no native stack.
        let mut stack: Vec<Open<'_>> = Vec::new();
        for root in roots {
            let open = inputs.open(
                root,
                0,
                &mut chain,
                &mut children,
                captures.len() as u32,
                reads.len() as u32,
            );
            stack.push(open);
            while let Some(open) = stack.last_mut() {
                match open.items.next() {
                    Some(Item::Capture(binding)) => {
                        let id = *binding_ids.entry(binding).or_insert_with(|| {
                            bindings.push(binding.clone());
                            last_capture.push(FIRST);
                            (bindings.len() - 1) as u32
                        });
                        let at = captures.len() as u32;
                        captures.push(CaptureOccurrence {
                            binding: id,
                            visible_from: chain
                                .get(&binding.defining_function)
                                .map_or(0, |depth| depth + 1),
                            previous: std::mem::replace(&mut last_capture[id as usize], at),
                        });
                    }
                    Some(Item::Read(read)) => {
                        let id = *binding_ids.entry(&read.binding).or_insert_with(|| {
                            bindings.push(read.binding.clone());
                            last_capture.push(FIRST);
                            (bindings.len() - 1) as u32
                        });
                        let at = reads.len() as u32;
                        let previous = last_read
                            .insert((id, read.path.as_ref()), at)
                            .unwrap_or(FIRST);
                        reads.push(ReadOccurrence {
                            read: read.clone(),
                            visible_from: chain
                                .get(&read.binding.defining_function)
                                .map_or(0, |depth| depth + 1),
                            previous,
                        });
                    }
                    Some(Item::Child(child)) => {
                        let depth = stack.len() as u32;
                        let open = inputs.open(
                            child,
                            depth,
                            &mut chain,
                            &mut children,
                            captures.len() as u32,
                            reads.len() as u32,
                        );
                        stack.push(open);
                    }
                    None => {
                        let open = stack.pop().expect("an open frame");
                        let entry = &entries[open.frame as usize];
                        match open.shadowed {
                            Some(depth) => {
                                chain.insert(&entry.key, depth);
                            }
                            None => {
                                chain.remove(&entry.key);
                            }
                        }
                        let own_exhaustive = entry.captures_exhaustive;
                        let exhaustive = own_exhaustive
                            && children[open.children.0 as usize..open.children.1 as usize]
                                .iter()
                                .all(|child| {
                                    frames[*child as usize]
                                        .as_ref()
                                        .is_some_and(|child| child.exhaustive)
                                });
                        frames[open.frame as usize] = Some(FrameSummary {
                            key: entry.key.clone(),
                            span: entry.span,
                            depth: stack.len() as u32,
                            captures: (open.captures_start, captures.len() as u32),
                            reads: (open.reads_start, reads.len() as u32),
                            children: open.children,
                            own_exhaustive,
                            exhaustive,
                        });
                    }
                }
            }
        }
        // A frame no outermost function reaches (a parent-link cycle in
        // resealed data) captures nothing.
        let frames = frames
            .into_iter()
            .zip(entries)
            .map(|(frame, entry)| {
                frame.unwrap_or_else(|| FrameSummary {
                    key: entry.key.clone(),
                    span: entry.span,
                    depth: 0,
                    captures: (0, 0),
                    reads: (0, 0),
                    children: (0, 0),
                    own_exhaustive: entry.captures_exhaustive,
                    exhaustive: entry.captures_exhaustive,
                })
            })
            .collect();
        Self {
            frames,
            children: children.into_boxed_slice(),
            bindings: bindings.into_boxed_slice(),
            captures: captures.into_boxed_slice(),
            reads: reads.into_boxed_slice(),
        }
    }

    /// The records this summary holds: one per frame, nested-frame link,
    /// distinct captured binding, capture occurrence and captured read.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    fn counts(&self) -> CaptureSummaryCounts {
        CaptureSummaryCounts {
            frames: self.frames.len(),
            nested_links: self.children.len(),
            bindings: self.bindings.len(),
            captures: self.captures.len(),
            reads: self.reads.len(),
        }
    }
}

/// The physical records one file's capture summary holds, for
/// measurement: each count grows with what the file authors, never with
/// how deeply its functions nest.
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureSummaryCounts {
    /// Function positions.
    pub frames: usize,
    /// Links from a function to a function nested directly in it.
    pub nested_links: usize,
    /// Distinct captured bindings: each identity is copied once.
    pub bindings: usize,
    /// Capture occurrences, each stored once where it is authored.
    pub captures: usize,
    /// Captured reads, each stored once where it is authored.
    pub reads: usize,
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
impl CaptureSummaryCounts {
    /// Every record, summed.
    #[must_use]
    pub fn total(&self) -> usize {
        self.frames + self.nested_links + self.bindings + self.captures + self.reads
    }
}

/// One served function position's handle on its file's capture summary.
#[derive(Clone)]
pub(super) struct FrameCaptureHandle {
    summary: Arc<CaptureSummaries>,
    frame: u32,
}

impl FrameCaptureHandle {
    pub(super) fn new(summary: &Arc<CaptureSummaries>, frame: usize) -> Self {
        Self {
            summary: Arc::clone(summary),
            frame: frame as u32,
        }
    }

    pub(super) fn view(&self) -> FunctionCaptures<'_> {
        FunctionCaptures {
            summary: &self.summary,
            frame: self.frame,
        }
    }

    /// Whether the function itself creates no callable no entry serves.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn own_exhaustive(&self) -> bool {
        self.view().own_exhaustive()
    }

    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(super) fn counts(&self) -> CaptureSummaryCounts {
        self.summary.counts()
    }

    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(super) fn summary_ptr(&self) -> *const () {
        Arc::as_ptr(&self.summary).cast()
    }
}

/// Two handles are equal when the functions' captures are: the same
/// captured bindings and reads, in the same order, the same exhaustiveness
/// and the same directly nested functions with theirs.
impl PartialEq for FrameCaptureHandle {
    fn eq(&self, other: &Self) -> bool {
        let (this, that) = (self.view(), other.view());
        this.shallow_eq(that)
            && this.own_exhaustive() == that.own_exhaustive()
            && this.nested().len() == that.nested().len()
            && this.nested().zip(that.nested()).all(|(child, other)| {
                child.function() == other.function()
                    && child.span() == other.span()
                    && child.shallow_eq(other)
            })
    }
}

impl Eq for FrameCaptureHandle {}

impl std::fmt::Debug for FrameCaptureHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.view().fmt(f)
    }
}

/// One function position's transitive captures, read from its file's
/// shared summary.
#[derive(Clone, Copy)]
pub struct FunctionCaptures<'a> {
    summary: &'a CaptureSummaries,
    frame: u32,
}

impl<'a> FunctionCaptures<'a> {
    fn frame(self) -> &'a FrameSummary {
        &self.summary.frames[self.frame as usize]
    }

    /// The function position.
    #[must_use]
    pub fn function(self) -> &'a FunctionProgramKey {
        &self.frame().key
    }

    /// The function's span: where it is created in the enclosing body.
    #[must_use]
    pub fn span(self) -> verter_span::Span {
        self.frame().span
    }

    /// Every binding the function or a function nested in it references
    /// from an enclosing frame, write-only ones included: each once, in
    /// first-reference source order (a nested function's at its start).
    #[must_use]
    pub fn bindings(self) -> CaptureBindings<'a> {
        let frame = self.frame();
        CaptureBindings {
            summary: self.summary,
            next: frame.captures.0,
            end: frame.captures.1,
            start: frame.captures.0,
            depth: frame.depth,
        }
    }

    /// Every read of an enclosing frame's binding by the function or a
    /// function nested in it, with its static path: each binding and path
    /// once, in source order.
    #[must_use]
    pub fn reads(self) -> CapturedReads<'a> {
        let frame = self.frame();
        CapturedReads {
            summary: self.summary,
            next: frame.reads.0,
            end: frame.reads.1,
            start: frame.reads.0,
            depth: frame.depth,
        }
    }

    /// Whether [`Self::bindings`] is exhaustive: `false` when the function,
    /// or a function nested in it, creates a callable no entry serves.
    #[must_use]
    pub fn exhaustive(self) -> bool {
        self.frame().exhaustive
    }

    /// The functions nested directly in this one, in source order of
    /// their discovery.
    #[must_use]
    pub fn nested(self) -> NestedCaptures<'a> {
        let (start, end) = self.frame().children;
        NestedCaptures {
            summary: self.summary,
            children: self.summary.children[start as usize..end as usize].iter(),
        }
    }

    fn own_exhaustive(self) -> bool {
        self.frame().own_exhaustive
    }

    fn shallow_eq(self, other: Self) -> bool {
        self.exhaustive() == other.exhaustive()
            && self.bindings().eq(other.bindings())
            && self.reads().eq(other.reads())
    }
}

impl std::fmt::Debug for FunctionCaptures<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionCaptures")
            .field("function", self.function())
            .field("span", &self.span())
            .field("bindings", &self.bindings().collect::<Vec<_>>())
            .field("reads", &self.reads().collect::<Vec<_>>())
            .field("exhaustive", &self.exhaustive())
            .finish()
    }
}

/// The functions nested directly in one function, with their captures.
#[derive(Clone)]
pub struct NestedCaptures<'a> {
    summary: &'a CaptureSummaries,
    children: std::slice::Iter<'a, u32>,
}

impl<'a> Iterator for NestedCaptures<'a> {
    type Item = FunctionCaptures<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.children.next().map(|&frame| FunctionCaptures {
            summary: self.summary,
            frame,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.children.size_hint()
    }
}

impl ExactSizeIterator for NestedCaptures<'_> {}

/// A function's captured bindings ([`FunctionCaptures::bindings`]).
#[derive(Clone)]
pub struct CaptureBindings<'a> {
    summary: &'a CaptureSummaries,
    next: u32,
    end: u32,
    start: u32,
    depth: u32,
}

impl<'a> Iterator for CaptureBindings<'a> {
    type Item = &'a FlowBindingIdentity;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next < self.end {
            let occurrence = &self.summary.captures[self.next as usize];
            self.next += 1;
            if occurrence.visible_from <= self.depth
                && (occurrence.previous == FIRST || occurrence.previous < self.start)
            {
                return Some(&self.summary.bindings[occurrence.binding as usize]);
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some((self.end - self.next) as usize))
    }
}

impl std::fmt::Debug for CaptureBindings<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

impl CaptureBindings<'_> {
    /// Whether the function captures nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.clone().next().is_none()
    }
}

/// A function's captured reads ([`FunctionCaptures::reads`]).
#[derive(Clone)]
pub struct CapturedReads<'a> {
    summary: &'a CaptureSummaries,
    next: u32,
    end: u32,
    start: u32,
    depth: u32,
}

impl<'a> Iterator for CapturedReads<'a> {
    type Item = &'a FunctionCapturedRead;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next < self.end {
            let occurrence = &self.summary.reads[self.next as usize];
            self.next += 1;
            if occurrence.visible_from <= self.depth
                && (occurrence.previous == FIRST || occurrence.previous < self.start)
            {
                return Some(&occurrence.read);
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some((self.end - self.next) as usize))
    }
}

impl std::fmt::Debug for CapturedReads<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

impl CapturedReads<'_> {
    /// Whether the function reads nothing from an enclosing frame.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.clone().next().is_none()
    }
}
