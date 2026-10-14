//! A program's file order: the sequence a TypeScript program lists its
//! source files in, which orders the declarations of a symbol merged across
//! files.
//!
//! TypeScript visits each root file in order and, before listing a file,
//! visits the files it pulls in — triple-slash references, then imports —
//! so a file lands AFTER everything it references (a post-order walk), and
//! the first visit of a file decides its place. Library files are listed
//! apart, before every program file; they never take a place here.

use std::hash::Hash;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::analysis::types::Hash16;
use crate::facts::fact_cache::FactVersionRef;
use crate::facts::version::RouteSurfaceFactRef;
use crate::facts::{FactKey, FactLane};

/// One program's file order, immutable once built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramFileOrder {
    /// The program's project config path; `None` for a file no configured
    /// program owns, whose contributors have no defined order.
    program: Option<Arc<str>>,
    ranks: FxHashMap<Arc<str>, u32>,
    fingerprint: Hash16,
}

impl ProgramFileOrder {
    /// The order of no program: every file ranks last, and nothing depends
    /// on it, so there is nothing to observe.
    #[must_use]
    pub fn unordered() -> Self {
        Self {
            program: None,
            ranks: FxHashMap::default(),
            fingerprint: [0; 16],
        }
    }

    /// Walk `roots` in order, placing each file after the files
    /// `references(file)` names (in order), first visit wins.
    /// `references` answers `None` for a file the program cannot load,
    /// which then takes no place.
    pub fn sequence(
        program: Arc<str>,
        roots: impl IntoIterator<Item = Arc<str>>,
        mut references: impl FnMut(&str) -> Option<Vec<Arc<str>>>,
    ) -> Self {
        let mut visited: FxHashSet<Arc<str>> = FxHashSet::default();
        let mut placed: Vec<Arc<str>> = Vec::new();
        // An explicit stack: a chain of imports thousands of files deep must
        // not overflow the thread's stack.
        let mut stack: Vec<(Arc<str>, Vec<Arc<str>>, usize)> = Vec::new();
        for root in roots {
            if !visited.insert(Arc::clone(&root)) {
                continue;
            }
            let Some(children) = references(&root) else {
                continue;
            };
            stack.push((root, children, 0));
            while let Some((_, children, next)) = stack.last_mut() {
                if let Some(child) = children.get(*next).cloned() {
                    *next += 1;
                    if visited.insert(Arc::clone(&child)) {
                        if let Some(grandchildren) = references(&child) {
                            stack.push((child, grandchildren, 0));
                        }
                    }
                } else if let Some((file, _, _)) = stack.pop() {
                    placed.push(file);
                }
            }
        }
        let fingerprint = crate::inputs::route_surface::hash16_from_sorted(|hasher| {
            program.hash(hasher);
            placed.hash(hasher);
        });
        let ranks = placed
            .into_iter()
            .enumerate()
            .map(|(rank, file)| (file, u32::try_from(rank).unwrap_or(u32::MAX - 1)))
            .collect();
        Self {
            program: Some(program),
            ranks,
            fingerprint,
        }
    }

    /// `canonical`'s place in the program, or `u32::MAX` for a file the
    /// program does not list (ties then break by canonical spelling).
    #[must_use]
    pub fn rank(&self, canonical: &str) -> u32 {
        self.ranks.get(canonical).copied().unwrap_or(u32::MAX)
    }

    /// How many files the program lists.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ranks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ranks.is_empty()
    }

    /// The program's project config path.
    #[must_use]
    pub fn program(&self) -> Option<&Arc<str>> {
        self.program.as_ref()
    }

    /// Fingerprint of the whole sequence.
    #[must_use]
    pub fn fingerprint(&self) -> Hash16 {
        self.fingerprint
    }

    /// The observation a result ordered by this program records: it misses
    /// once the program lists its files differently. `None` for
    /// [`Self::unordered`].
    #[must_use]
    pub fn fact(&self) -> Option<FactVersionRef> {
        let program = self.program.as_ref()?;
        Some(FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: String::new(),
            key: FactKey::ProgramFileOrder {
                program: Arc::clone(program),
            },
            lane: FactLane::Semantic,
            expected_hash: self.fingerprint,
        }))
    }
}

/// Whether ordering `canonicals` needs a program order: only two or more
/// distinct program files can be ordered by one. Library files always
/// precede program files, so they never need it.
pub fn orders_across_files<'c>(canonicals: impl IntoIterator<Item = &'c str>) -> bool {
    let mut first: Option<&str> = None;
    for canonical in canonicals {
        if crate::inputs::contributors::is_automatic_lib_canonical(canonical) {
            continue;
        }
        match first {
            None => first = Some(canonical),
            Some(seen) if seen != canonical => return true,
            Some(_) => {}
        }
    }
    false
}

#[cfg(test)]
#[path = "program_order_tests.rs"]
mod tests;
