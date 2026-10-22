//! A program's file order: the sequence a TypeScript program lists its
//! source files in, which orders the declarations of a symbol merged across
//! files.
//!
//! TypeScript visits each root file in order and, before listing a file,
//! visits the files it pulls in — triple-slash references, then imports —
//! so a file lands AFTER everything it references (a post-order walk), and
//! the first visit of a file decides its place. Library files take part in
//! the same walk (a `/// <reference lib>` directive, the configured `lib`
//! selection and a library's own references reach them) but are listed
//! apart, before every program file, by TypeScript's library priority
//! (`compareDefaultLibFiles`): the order of its library table, then the
//! order the walk reached them.

use std::hash::Hash;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::analysis::types::Hash16;
use crate::facts::fact_cache::FactVersionRef;
use crate::facts::version::RouteSurfaceFactRef;
use crate::facts::{FactKey, FactLane};
use crate::inputs::contributors::is_automatic_lib_canonical;

/// TypeScript's library table (`libEntries`), in its order: each
/// `/// <reference lib>` / `lib` name and the file it loads. The order is
/// the library priority (`getDefaultLibFilePriority`).
const LIBRARIES: &[(&str, &str)] = &[
    ("es5", "lib.es5.d.ts"),
    ("es6", "lib.es2015.d.ts"),
    ("es2015", "lib.es2015.d.ts"),
    ("es7", "lib.es2016.d.ts"),
    ("es2016", "lib.es2016.d.ts"),
    ("es2017", "lib.es2017.d.ts"),
    ("es2018", "lib.es2018.d.ts"),
    ("es2019", "lib.es2019.d.ts"),
    ("es2020", "lib.es2020.d.ts"),
    ("es2021", "lib.es2021.d.ts"),
    ("es2022", "lib.es2022.d.ts"),
    ("es2023", "lib.es2023.d.ts"),
    ("es2024", "lib.es2024.d.ts"),
    ("es2025", "lib.es2025.d.ts"),
    ("esnext", "lib.esnext.d.ts"),
    ("dom", "lib.dom.d.ts"),
    ("dom.iterable", "lib.dom.iterable.d.ts"),
    ("dom.asynciterable", "lib.dom.asynciterable.d.ts"),
    ("webworker", "lib.webworker.d.ts"),
    (
        "webworker.importscripts",
        "lib.webworker.importscripts.d.ts",
    ),
    ("webworker.iterable", "lib.webworker.iterable.d.ts"),
    (
        "webworker.asynciterable",
        "lib.webworker.asynciterable.d.ts",
    ),
    ("scripthost", "lib.scripthost.d.ts"),
    ("es2015.core", "lib.es2015.core.d.ts"),
    ("es2015.collection", "lib.es2015.collection.d.ts"),
    ("es2015.generator", "lib.es2015.generator.d.ts"),
    ("es2015.iterable", "lib.es2015.iterable.d.ts"),
    ("es2015.promise", "lib.es2015.promise.d.ts"),
    ("es2015.proxy", "lib.es2015.proxy.d.ts"),
    ("es2015.reflect", "lib.es2015.reflect.d.ts"),
    ("es2015.symbol", "lib.es2015.symbol.d.ts"),
    (
        "es2015.symbol.wellknown",
        "lib.es2015.symbol.wellknown.d.ts",
    ),
    ("es2016.array.include", "lib.es2016.array.include.d.ts"),
    ("es2016.intl", "lib.es2016.intl.d.ts"),
    ("es2017.arraybuffer", "lib.es2017.arraybuffer.d.ts"),
    ("es2017.date", "lib.es2017.date.d.ts"),
    ("es2017.object", "lib.es2017.object.d.ts"),
    ("es2017.sharedmemory", "lib.es2017.sharedmemory.d.ts"),
    ("es2017.string", "lib.es2017.string.d.ts"),
    ("es2017.intl", "lib.es2017.intl.d.ts"),
    ("es2017.typedarrays", "lib.es2017.typedarrays.d.ts"),
    ("es2018.asyncgenerator", "lib.es2018.asyncgenerator.d.ts"),
    ("es2018.asynciterable", "lib.es2018.asynciterable.d.ts"),
    ("es2018.intl", "lib.es2018.intl.d.ts"),
    ("es2018.promise", "lib.es2018.promise.d.ts"),
    ("es2018.regexp", "lib.es2018.regexp.d.ts"),
    ("es2019.array", "lib.es2019.array.d.ts"),
    ("es2019.object", "lib.es2019.object.d.ts"),
    ("es2019.string", "lib.es2019.string.d.ts"),
    ("es2019.symbol", "lib.es2019.symbol.d.ts"),
    ("es2019.intl", "lib.es2019.intl.d.ts"),
    ("es2020.bigint", "lib.es2020.bigint.d.ts"),
    ("es2020.date", "lib.es2020.date.d.ts"),
    ("es2020.promise", "lib.es2020.promise.d.ts"),
    ("es2020.sharedmemory", "lib.es2020.sharedmemory.d.ts"),
    ("es2020.string", "lib.es2020.string.d.ts"),
    (
        "es2020.symbol.wellknown",
        "lib.es2020.symbol.wellknown.d.ts",
    ),
    ("es2020.intl", "lib.es2020.intl.d.ts"),
    ("es2020.number", "lib.es2020.number.d.ts"),
    ("es2021.promise", "lib.es2021.promise.d.ts"),
    ("es2021.string", "lib.es2021.string.d.ts"),
    ("es2021.weakref", "lib.es2021.weakref.d.ts"),
    ("es2021.intl", "lib.es2021.intl.d.ts"),
    ("es2022.array", "lib.es2022.array.d.ts"),
    ("es2022.error", "lib.es2022.error.d.ts"),
    ("es2022.intl", "lib.es2022.intl.d.ts"),
    ("es2022.object", "lib.es2022.object.d.ts"),
    ("es2022.string", "lib.es2022.string.d.ts"),
    ("es2022.regexp", "lib.es2022.regexp.d.ts"),
    ("es2023.array", "lib.es2023.array.d.ts"),
    ("es2023.collection", "lib.es2023.collection.d.ts"),
    ("es2023.intl", "lib.es2023.intl.d.ts"),
    ("es2024.arraybuffer", "lib.es2024.arraybuffer.d.ts"),
    ("es2024.collection", "lib.es2024.collection.d.ts"),
    ("es2024.object", "lib.es2024.object.d.ts"),
    ("es2024.promise", "lib.es2024.promise.d.ts"),
    ("es2024.regexp", "lib.es2024.regexp.d.ts"),
    ("es2024.sharedmemory", "lib.es2024.sharedmemory.d.ts"),
    ("es2024.string", "lib.es2024.string.d.ts"),
    ("es2025.collection", "lib.es2025.collection.d.ts"),
    ("es2025.float16", "lib.es2025.float16.d.ts"),
    ("es2025.intl", "lib.es2025.intl.d.ts"),
    ("es2025.iterator", "lib.es2025.iterator.d.ts"),
    ("es2025.promise", "lib.es2025.promise.d.ts"),
    ("es2025.regexp", "lib.es2025.regexp.d.ts"),
    ("esnext.asynciterable", "lib.es2018.asynciterable.d.ts"),
    ("esnext.symbol", "lib.es2019.symbol.d.ts"),
    ("esnext.bigint", "lib.es2020.bigint.d.ts"),
    ("esnext.weakref", "lib.es2021.weakref.d.ts"),
    ("esnext.object", "lib.es2024.object.d.ts"),
    ("esnext.regexp", "lib.es2024.regexp.d.ts"),
    ("esnext.string", "lib.es2024.string.d.ts"),
    ("esnext.float16", "lib.es2025.float16.d.ts"),
    ("esnext.iterator", "lib.es2025.iterator.d.ts"),
    ("esnext.promise", "lib.es2025.promise.d.ts"),
    ("esnext.array", "lib.esnext.array.d.ts"),
    ("esnext.collection", "lib.esnext.collection.d.ts"),
    ("esnext.date", "lib.esnext.date.d.ts"),
    ("esnext.decorators", "lib.esnext.decorators.d.ts"),
    ("esnext.disposable", "lib.esnext.disposable.d.ts"),
    ("esnext.error", "lib.esnext.error.d.ts"),
    ("esnext.intl", "lib.esnext.intl.d.ts"),
    ("esnext.sharedmemory", "lib.esnext.sharedmemory.d.ts"),
    ("esnext.temporal", "lib.esnext.temporal.d.ts"),
    ("esnext.typedarrays", "lib.esnext.typedarrays.d.ts"),
    ("decorators", "lib.decorators.d.ts"),
    ("decorators.legacy", "lib.decorators.legacy.d.ts"),
];

/// The library file `/// <reference lib="reference" />` loads
/// (`getLibFileNameFromLibReference`), or `None` for a name TypeScript
/// does not know, which loads nothing.
#[must_use]
pub fn library_file_name(reference: &str) -> Option<&'static str> {
    let name = reference.trim().to_ascii_lowercase();
    LIBRARIES
        .iter()
        .find(|(entry, _)| *entry == name)
        .map(|(_, file)| *file)
}

/// A library file's priority (`getDefaultLibFilePriority`): `lib.d.ts`
/// and `lib.es6.d.ts` first, then the files the library table names, in
/// its order, then every other library.
#[must_use]
pub fn library_priority(canonical: &str) -> u32 {
    let file = canonical.rsplit(['/', '\\']).next().unwrap_or(canonical);
    if file == "lib.d.ts" || file == "lib.es6.d.ts" {
        return 0;
    }
    let unknown = LIBRARIES.len() + 2;
    let position = file
        .strip_prefix("lib.")
        .and_then(|name| name.strip_suffix(".d.ts"))
        .and_then(|name| LIBRARIES.iter().position(|(entry, _)| *entry == name))
        .map_or(unknown, |index| index + 1);
    u32::try_from(position).unwrap_or(u32::MAX)
}

/// One program's file order, immutable once built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramFileOrder {
    /// The program's project config path; `None` for an order no
    /// configured program defines, which nothing observes.
    program: Option<Arc<str>>,
    ranks: FxHashMap<Arc<str>, u32>,
    /// Whether the order lists every library the program loads, so a
    /// library it does not list is not loaded. `false` only for
    /// [`Self::unordered`].
    knows_libraries: bool,
    fingerprint: Hash16,
}

impl ProgramFileOrder {
    /// The order of no program: every file ranks last, every library
    /// counts as loaded, and nothing depends on it, so there is nothing
    /// to observe.
    #[must_use]
    pub fn unordered() -> Self {
        Self {
            program: None,
            ranks: FxHashMap::default(),
            knows_libraries: false,
            fingerprint: [0; 16],
        }
    }

    /// Walk `roots` in order, placing each file after the files
    /// `references(file)` names (in order), first visit wins; the library
    /// files the walk reaches are listed first, by [`library_priority`].
    /// `references` answers `None` for a file the program cannot load,
    /// which then takes no place.
    pub fn sequence(
        program: Arc<str>,
        roots: impl IntoIterator<Item = Arc<str>>,
        references: impl FnMut(&str) -> Option<Vec<Arc<str>>>,
    ) -> Self {
        Self::walk(Some(program), roots, references)
    }

    /// The libraries `roots` and their references reach, by
    /// [`library_priority`] then visit order: the part of a program's
    /// order that its configuration and libraries decide, without its
    /// source files. Nothing observes it.
    pub fn libraries(
        roots: impl IntoIterator<Item = Arc<str>>,
        references: impl FnMut(&str) -> Option<Vec<Arc<str>>>,
    ) -> Self {
        Self::walk(None, roots, references)
    }

    fn walk(
        program: Option<Arc<str>>,
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
        // `files = toSorted(defaultLibFiles, compareDefaultLibFiles)
        // .concat(otherFiles)`, a stable sort.
        let (mut libraries, files): (Vec<Arc<str>>, Vec<Arc<str>>) = placed
            .into_iter()
            .partition(|file| is_automatic_lib_canonical(file));
        libraries.sort_by_key(|library| library_priority(library));
        let sequence: Vec<Arc<str>> = libraries.into_iter().chain(files).collect();
        let fingerprint = crate::inputs::route_surface::hash16_from_sorted(|hasher| {
            program.hash(hasher);
            sequence.hash(hasher);
        });
        let ranks = sequence
            .into_iter()
            .enumerate()
            .map(|(rank, file)| (file, u32::try_from(rank).unwrap_or(u32::MAX - 1)))
            .collect();
        Self {
            program,
            ranks,
            knows_libraries: true,
            fingerprint,
        }
    }

    /// `canonical`'s place in the program, or `u32::MAX` for a file the
    /// program does not list (ties then break by canonical spelling).
    #[must_use]
    pub fn rank(&self, canonical: &str) -> u32 {
        self.ranks.get(canonical).copied().unwrap_or(u32::MAX)
    }

    /// Whether the program lists `canonical`.
    #[must_use]
    pub fn lists(&self, canonical: &str) -> bool {
        self.ranks.contains_key(canonical)
    }

    /// Whether the program loads the library `canonical`: it lists it, or
    /// the order does not know the program's libraries.
    #[must_use]
    pub fn loads_library(&self, canonical: &str) -> bool {
        !self.knows_libraries || self.lists(canonical)
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
    /// once the program lists its files differently. `None` for an order no
    /// program defines.
    #[must_use]
    pub fn fact(&self) -> Option<FactVersionRef> {
        let program = self.program.as_ref()?;
        Some(FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: Arc::default(),
            key: FactKey::ProgramFileOrder {
                program: Arc::clone(program),
            },
            lane: FactLane::Semantic,
            expected_hash: self.fingerprint,
        }))
    }
}

/// Whether ordering `canonicals` needs the program's whole order rather
/// than the order its libraries alone decide (`libraries`, from
/// [`ProgramFileOrder::libraries`]): they span two or more distinct
/// program files, or name a library `libraries` does not list — one a
/// program file's `/// <reference lib>` may load, which only a walk of the
/// program's files can tell.
pub fn needs_program_walk<'c>(
    canonicals: impl IntoIterator<Item = &'c str>,
    libraries: &ProgramFileOrder,
) -> bool {
    let mut first: Option<&str> = None;
    for canonical in canonicals {
        if is_automatic_lib_canonical(canonical) {
            if !libraries.lists(canonical) {
                return true;
            }
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

/// Whether ordering `canonicals` needs a program order: two or more
/// distinct program files. Library files are not counted.
pub fn orders_across_files<'c>(canonicals: impl IntoIterator<Item = &'c str>) -> bool {
    let mut first: Option<&str> = None;
    for canonical in canonicals {
        if is_automatic_lib_canonical(canonical) {
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
