//! The per-file class index: every class one parsed file authors, recorded
//! syntactically by the function-program discovery walk that indexes the
//! file, and prepared once into keyed lookups when the index is sealed.
//!
//! A class is addressed by its node span. The index answers, each by lookup
//! rather than by a scan of the classes:
//!
//! - which class declares a member directly (a member's declaring class);
//! - which class lexically encloses a class (its nearest enclosing class,
//!   across function bodies);
//! - which class this file authors a class's `extends` clause names, as the
//!   discovery walk resolved it lexically.

use std::sync::Arc;

/// One class the file authors — a declaration at any depth or a class
/// expression — recorded syntactically at index time. A member's
/// declaring class is the class whose body declares it directly, so a
/// class records the span of each member it declares: each class element,
/// and each constructor parameter that declares a property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSyntaxRecord {
    /// The class node's span.
    pub span: verter_span::Span,
    /// Whether the class is an expression rather than a declaration.
    pub expression: bool,
    /// Whether the class has an `extends` clause.
    pub has_heritage: bool,
    /// The span of each member the class declares directly, in source
    /// order.
    pub members: Arc<[verter_span::Span]>,
}

/// What a class's `extends` clause names, as the discovery walk resolved
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassBase {
    /// The class has no `extends` clause.
    None,
    /// The clause is a bare name that binds, lexically, a class this file
    /// authors: a class declaration, or a `const` initialized with a class
    /// expression. The value is the base class's position in the
    /// discovery's class list.
    Class(u32),
    /// The clause names something else: a call (a mixin application), a
    /// member access, a name binding anything but a class this file
    /// authors, or a name whose binding the walk cannot read.
    Unresolved,
}

/// One class as the discovery walk hands it to the index: its syntactic
/// record and its resolved base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSyntaxDiscovery {
    pub record: ClassSyntaxRecord,
    pub base: ClassBase,
}

/// The sealed class index of one file. Built once by
/// [`ClassIndex::from_discovery`]; every lookup is a keyed read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassIndex {
    /// Every class, in source order.
    records: Arc<[ClassSyntaxRecord]>,
    /// Each class's resolved base, by position.
    bases: Arc<[ClassBase]>,
    /// Each class's nearest lexically enclosing class, by position.
    parents: Arc<[Option<u32>]>,
    /// The class declaring each member directly, by member span.
    by_member: Arc<rustc_hash::FxHashMap<verter_span::Span, u32>>,
    /// Each class's position, by its span.
    by_span: Arc<rustc_hash::FxHashMap<verter_span::Span, u32>>,
}

/// One class of a [`ClassIndex`], with the index it was read from.
#[derive(Debug, Clone, Copy)]
pub struct ClassIndexMatch<'a> {
    index: &'a ClassIndex,
    position: u32,
}

impl<'a> ClassIndexMatch<'a> {
    /// The class's syntactic record.
    #[must_use]
    pub fn record(&self) -> &'a ClassSyntaxRecord {
        &self.index.records[self.position as usize]
    }

    /// Whether another class of the file lexically encloses this one.
    #[must_use]
    pub fn is_enclosed(&self) -> bool {
        self.index.parents[self.position as usize].is_some()
    }

    /// The class this one's `extends` clause names, as the discovery walk
    /// resolved it.
    #[must_use]
    pub fn base(&self) -> ClassBaseMatch<'a> {
        match self.index.bases[self.position as usize] {
            ClassBase::None => ClassBaseMatch::None,
            ClassBase::Class(position) => ClassBaseMatch::Class(ClassIndexMatch {
                index: self.index,
                position,
            }),
            ClassBase::Unresolved => ClassBaseMatch::Unresolved,
        }
    }
}

/// A class's base as read from a [`ClassIndex`] ([`ClassBase`], with the
/// base class matched in the same index).
#[derive(Debug, Clone, Copy)]
pub enum ClassBaseMatch<'a> {
    /// No `extends` clause.
    None,
    /// The class this file authors that the clause names.
    Class(ClassIndexMatch<'a>),
    /// The clause names something else.
    Unresolved,
}

impl ClassIndex {
    /// Seal the discovered classes: sort them into source order (outer
    /// before inner), derive each class's enclosing class from span
    /// containment, and key members and classes by span. A base position
    /// the discovery recorded refers to the discovery's own order and is
    /// remapped here; one that names no class reads as unresolved, and a
    /// member span claimed by two classes stays with the first in source
    /// order.
    #[must_use]
    pub fn from_discovery(classes: Vec<ClassSyntaxDiscovery>) -> Self {
        let mut order: Vec<usize> = (0..classes.len()).collect();
        order.sort_by_key(|&at| {
            let span = classes[at].record.span;
            (span.start, std::cmp::Reverse(span.end))
        });
        let mut position_of = vec![u32::MAX; classes.len()];
        for (position, &at) in order.iter().enumerate() {
            position_of[at] = u32::try_from(position).unwrap_or(u32::MAX);
        }
        let mut records = Vec::with_capacity(classes.len());
        let mut bases = Vec::with_capacity(classes.len());
        for &at in &order {
            let class = &classes[at];
            records.push(class.record.clone());
            bases.push(match class.base {
                ClassBase::Class(base) => position_of
                    .get(base as usize)
                    .copied()
                    .filter(|&position| position != u32::MAX)
                    .map_or(ClassBase::Unresolved, ClassBase::Class),
                other => other,
            });
        }
        // Class spans are laminar: in source order a stack of the classes
        // still open at each start holds exactly the enclosing ones.
        let mut parents = Vec::with_capacity(records.len());
        let mut open: Vec<u32> = Vec::new();
        let mut by_member = rustc_hash::FxHashMap::default();
        let mut by_span = rustc_hash::FxHashMap::default();
        for (position, record) in records.iter().enumerate() {
            let position = u32::try_from(position).unwrap_or(u32::MAX);
            while open.last().is_some_and(|&outer| {
                let outer = records[outer as usize].span;
                !(outer.start <= record.span.start && record.span.end <= outer.end)
            }) {
                open.pop();
            }
            parents.push(open.last().copied());
            open.push(position);
            by_span.entry(record.span).or_insert(position);
            for member in record.members.iter() {
                by_member.entry(*member).or_insert(position);
            }
        }
        Self {
            records: Arc::from(records.into_boxed_slice()),
            bases: Arc::from(bases.into_boxed_slice()),
            parents: Arc::from(parents.into_boxed_slice()),
            by_member: Arc::new(by_member),
            by_span: Arc::new(by_span),
        }
    }

    /// The class whose body declares a member directly at `declaration`,
    /// the member's declaration span: a class element's span, or a
    /// property-declaring constructor parameter's.
    #[must_use]
    pub fn declaring_member(&self, declaration: verter_span::Span) -> Option<ClassIndexMatch<'_>> {
        let position = *self.by_member.get(&declaration)?;
        Some(ClassIndexMatch {
            index: self,
            position,
        })
    }

    /// The class whose node is at `span`.
    #[must_use]
    pub fn at(&self, span: verter_span::Span) -> Option<ClassIndexMatch<'_>> {
        let position = *self.by_span.get(&span)?;
        Some(ClassIndexMatch {
            index: self,
            position,
        })
    }

    /// Every class, in source order.
    #[must_use]
    pub fn records(&self) -> &[ClassSyntaxRecord] {
        &self.records
    }
}
