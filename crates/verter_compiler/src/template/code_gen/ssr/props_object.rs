//! Ordered key/value assembly for the SSR emitter's JS object literals.
//!
//! SSR builds two object literals per element: the `_mergeProps` attrs object
//! for HTML elements and the props object for components. Both need the same
//! four things — source-order placement, a position held open for `class` and
//! `style` (whose final merged value is only known after every prop has been
//! visited), duplicate-key merging into an array value, and the byte offset of
//! one key inside the emitted text so the caller can anchor a source mapping.
//!
//! Keeping the key, the value and their order as separate fields until a single
//! `render` makes each of those an indexing operation. The alternative — emit
//! the pairs into one buffer and recover structure afterwards — has to re-read
//! its own output to find a key, and a whole-buffer substitution cannot tell
//! two occurrences of the same reserved position apart.

/// A position reserved in a [`PropsObject`] before its value is known.
///
/// A slot that is never filled renders nothing at all, so an unresolved
/// `class`/`style` reservation cannot leak into the emitted object.
#[derive(Clone, Copy)]
pub(super) struct PropsSlot(usize);

/// Source position used by entries that carry no authored position of their
/// own; sorts after every real position, matching "append at the end".
const APPENDED: u32 = u32::MAX;

struct PropsEntry {
    /// Rendered key token, including the quotes when the key needs them.
    /// Quoting is part of the key's identity: `"foo"` and `foo` are emitted as
    /// authored and are not merged with each other.
    key: String,
    value: String,
    /// Authored source byte position of the prop this entry came from, used to
    /// place later source-ordered insertions.
    source_pos: u32,
    /// When set, [`PropsObject::render`] reports this entry's key offset paired
    /// with this authored source position.
    anchor: Option<u32>,
}

/// An ordered `key: value` set rendered exactly once.
#[derive(Default)]
pub(super) struct PropsObject {
    /// `None` is a reserved-but-unfilled slot.
    entries: Vec<Option<PropsEntry>>,
}

impl PropsObject {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// True when nothing would be rendered — reserved slots that were never
    /// filled do not count.
    pub(super) fn is_empty(&self) -> bool {
        !self.entries.iter().any(|e| e.is_some())
    }

    /// Append an entry that carries no authored position.
    pub(super) fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.push_at(key, value, APPENDED);
    }

    /// Append an entry authored at `source_pos`.
    pub(super) fn push_at(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        source_pos: u32,
    ) {
        self.entries.push(Some(PropsEntry {
            key: key.into(),
            value: value.into(),
            source_pos,
            anchor: None,
        }));
    }

    /// Insert an entry authored at `source_pos` before the first entry authored
    /// later than it.
    pub(super) fn insert_by_source_order(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        source_pos: u32,
    ) {
        let index = self
            .entries
            .iter()
            .position(|e| {
                e.as_ref()
                    .is_some_and(|entry| entry.source_pos >= source_pos)
            })
            .unwrap_or(self.entries.len());
        self.entries.insert(
            index,
            Some(PropsEntry {
                key: key.into(),
                value: value.into(),
                source_pos,
                anchor: None,
            }),
        );
    }

    /// Hold the current position for a value decided later.
    pub(super) fn reserve(&mut self) -> PropsSlot {
        self.entries.push(None);
        PropsSlot(self.entries.len() - 1)
    }

    /// Fill `slot` if one was reserved, otherwise append.
    pub(super) fn fill_or_push(
        &mut self,
        slot: Option<PropsSlot>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) {
        let entry = PropsEntry {
            key: key.into(),
            value: value.into(),
            source_pos: APPENDED,
            anchor: None,
        };
        match slot {
            Some(PropsSlot(index)) => self.entries[index] = Some(entry),
            None => self.entries.push(Some(entry)),
        }
    }

    /// Fill `slot` (or append) and mark the entry's key as the one whose
    /// rendered offset [`PropsObject::render`] should report.
    pub(super) fn fill_or_push_anchored(
        &mut self,
        slot: Option<PropsSlot>,
        key: impl Into<String>,
        value: impl Into<String>,
        source_pos: u32,
    ) {
        self.fill_or_push(slot, key, value);
        let index = match slot {
            Some(PropsSlot(index)) => index,
            None => self.entries.len() - 1,
        };
        if let Some(entry) = &mut self.entries[index] {
            entry.anchor = Some(source_pos);
        }
    }

    /// Take the entries out, leaving an empty object behind.
    pub(super) fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    /// Collapse entries that repeat a key into one entry holding an array of
    /// every value, in authoring order, at the first occurrence's position.
    ///
    /// A component can name the same key twice — `v-model` and an explicit
    /// `@update:model-value` both emit `"onUpdate:modelValue"` — and Vue passes
    /// both handlers as `[first, second]` rather than dropping one.
    pub(super) fn merge_duplicate_keys(&mut self) {
        if self.entries.iter().filter(|e| e.is_some()).count() < 2 {
            return;
        }

        // (key, indices) in first-occurrence order; a props object holds a
        // handful of entries, so a linear scan beats hashing here.
        let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
        for (index, entry) in self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|entry| (i, entry)))
        {
            match groups.iter_mut().find(|(key, _)| *key == entry.key) {
                Some((_, indices)) => indices.push(index),
                None => groups.push((entry.key.as_str(), vec![index])),
            }
        }

        let merges: Vec<(usize, String, Vec<usize>)> = groups
            .into_iter()
            .filter(|(_, indices)| indices.len() > 1)
            .map(|(_, indices)| {
                let values: Vec<&str> = indices
                    .iter()
                    .map(|&i| {
                        self.entries[i]
                            .as_ref()
                            .expect("grouped entry")
                            .value
                            .as_str()
                    })
                    .collect();
                let merged = format!("[{}]", values.join(", "));
                (indices[0], merged, indices[1..].to_vec())
            })
            .collect();

        for (first, merged, duplicates) in merges {
            if let Some(entry) = &mut self.entries[first] {
                entry.value = merged;
            }
            for index in duplicates {
                self.entries[index] = None;
            }
        }
    }

    /// Render `key: value, key: value`, reporting the anchored key's
    /// `(offset in the returned string, authored source position)` when one
    /// entry asked for it.
    pub(super) fn render(&self) -> (String, Option<(u32, u32)>) {
        let mut out = String::new();
        let mut anchor = None;
        for entry in self.entries.iter().flatten() {
            if !out.is_empty() {
                out.push_str(", ");
            }
            if let Some(source_pos) = entry.anchor {
                anchor = Some((out.len() as u32, source_pos));
            }
            out.push_str(&entry.key);
            out.push_str(": ");
            out.push_str(&entry.value);
        }
        (out, anchor)
    }

    /// Render without an anchor, for the callers that never set one.
    pub(super) fn render_body(&self) -> String {
        self.render().0
    }
}
