//! Validated mapping from one parsed program's top-level statements to their
//! neutral lexical owners.
//!
//! Framework/session code classifies authored regions before semantic work and
//! supplies only [`TopLevelOwnerId`] values. This module derives the stable
//! owner-local statement ordinal exactly once. Semantic producers consume the
//! resulting table by statement index; they never recover ownership from spans
//! or source text.

use std::ops::Index;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_no_typeexpr::NoTypeExpr;
use verter_span::Span;
use verter_type_expr::{DeclKey, TopLevelOwnerId, TopLevelOwnerKind};

#[inline]
pub(crate) fn checked_authored_ordinal(index: usize) -> Option<u32> {
    u32::try_from(index).ok()
}

/// Key view accepted by [`DeclMap`]. A bare name is explicitly the ordinary
/// `Module(0)` compatibility view; owner-aware callers pass a [`DeclKey`].
pub trait DeclMapKey {
    fn owner(&self) -> TopLevelOwnerId;
    fn name(&self) -> &str;
}

impl DeclMapKey for str {
    fn owner(&self) -> TopLevelOwnerId {
        TopLevelOwnerId::ordinary_file()
    }

    fn name(&self) -> &str {
        self
    }
}

impl DeclMapKey for DeclKey {
    fn owner(&self) -> TopLevelOwnerId {
        self.owner
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Single-authority declaration map keyed canonically by `(owner, name)`.
#[derive(Debug, Clone, NoTypeExpr)]
pub struct DeclMap<V> {
    entries: FxHashMap<DeclKey, V>,
}

impl<V> Default for DeclMap<V> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
        }
    }
}

impl<V> DeclMap<V> {
    #[must_use]
    pub fn get<Q: DeclMapKey + ?Sized>(&self, key: &Q) -> Option<&V> {
        self.entries.get(&DeclKey::new(key.owner(), key.name()))
    }

    pub fn get_mut<Q: DeclMapKey + ?Sized>(&mut self, key: &Q) -> Option<&mut V> {
        self.entries.get_mut(&DeclKey::new(key.owner(), key.name()))
    }

    #[must_use]
    pub fn contains_key<Q: DeclMapKey + ?Sized>(&self, key: &Q) -> bool {
        self.get(key).is_some()
    }

    pub fn entry(&mut self, key: DeclKey) -> std::collections::hash_map::Entry<'_, DeclKey, V> {
        self.entries.entry(key)
    }

    pub fn insert(&mut self, key: DeclKey, value: V) -> Option<V> {
        self.entries.insert(key, value)
    }

    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, DeclKey, V> {
        self.entries.iter()
    }

    pub fn keys(&self) -> std::collections::hash_map::Keys<'_, DeclKey, V> {
        self.entries.keys()
    }

    pub fn values(&self) -> std::collections::hash_map::Values<'_, DeclKey, V> {
        self.entries.values()
    }

    pub fn values_mut(&mut self) -> std::collections::hash_map::ValuesMut<'_, DeclKey, V> {
        self.entries.values_mut()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a, V> IntoIterator for &'a DeclMap<V> {
    type Item = (&'a DeclKey, &'a V);
    type IntoIter = std::collections::hash_map::Iter<'a, DeclKey, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

impl<V> Index<&str> for DeclMap<V> {
    type Output = V;

    fn index(&self, name: &str) -> &Self::Output {
        self.get(name)
            .unwrap_or_else(|| panic!("ordinary declaration `{name}` is not indexed"))
    }
}

impl<V> Index<&DeclKey> for DeclMap<V> {
    type Output = V;

    fn index(&self, key: &DeclKey) -> &Self::Output {
        self.get(key)
            .unwrap_or_else(|| panic!("declaration `{key:?}` is not indexed"))
    }
}

/// Owner coordinates of one `Program.body` statement.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, NoTypeExpr,
)]
pub struct TopLevelStatementOwner {
    pub owner: TopLevelOwnerId,
    pub owner_local_ordinal: u32,
}

/// Owner resolution for a parser-authored comment attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct TopLevelAttachedOwner {
    pub owner: TopLevelOwnerId,
    pub statement_index: Option<u32>,
    pub owner_local_ordinal: Option<u32>,
}

/// Explicit authored source region owned by one neutral top-level owner.
/// Carrier/session code supplies these ranges; semantic code never infers
/// framework roles from source text or hard-coded block kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct TopLevelOwnerRegion {
    pub owner: TopLevelOwnerId,
    pub span: Span,
}

/// Invalid explicit owner-region table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevelOwnerRegionError {
    EmptyRegion { index: usize },
    OverlappingRegions { previous: usize, next: usize },
}

impl std::fmt::Display for TopLevelOwnerRegionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::EmptyRegion { index } => write!(formatter, "owner region {index} is empty"),
            Self::OverlappingRegions { previous, next } => {
                write!(formatter, "owner regions {previous} and {next} overlap")
            }
        }
    }
}

impl std::error::Error for TopLevelOwnerRegionError {}

/// Error returned when a caller's owner mapping does not cover the parsed
/// program exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevelOwnerTableError {
    LengthMismatch {
        statement_count: usize,
        owner_count: usize,
    },
    StatementCountOverflow {
        statement_count: usize,
    },
    OwnerLocalOrdinalOverflow {
        owner: TopLevelOwnerId,
    },
}

impl std::fmt::Display for TopLevelOwnerTableError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::LengthMismatch {
                statement_count,
                owner_count,
            } => write!(
                formatter,
                "top-level owner mapping length mismatch: program has {statement_count} statements, mapping has {owner_count}"
            ),
            Self::StatementCountOverflow { statement_count } => write!(
                formatter,
                "top-level statement count {statement_count} exceeds the u32 authored-ordinal domain"
            ),
            Self::OwnerLocalOrdinalOverflow { owner } => write!(
                formatter,
                "top-level owner {owner:?} exceeds the u32 owner-local ordinal domain"
            ),
        }
    }
}

impl std::error::Error for TopLevelOwnerTableError {}

/// Validated, immutable owner coordinates parallel to `Program.body`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct TopLevelOwnerTable {
    statements: Arc<[TopLevelStatementOwner]>,
    regions: Arc<[TopLevelOwnerRegion]>,
    fallback_owner: Option<TopLevelOwnerId>,
}

impl TopLevelOwnerTable {
    /// Validate an explicit statement-owner mapping and derive stable
    /// zero-based ordinals independently within each owner.
    pub fn try_from_statement_owners<I>(
        statement_count: usize,
        owners: I,
    ) -> Result<Self, TopLevelOwnerTableError>
    where
        I: IntoIterator<Item = TopLevelOwnerId>,
    {
        if checked_authored_ordinal(statement_count).is_none() {
            return Err(TopLevelOwnerTableError::StatementCountOverflow { statement_count });
        }
        let owners = owners.into_iter().collect::<Vec<_>>();
        if owners.len() != statement_count {
            return Err(TopLevelOwnerTableError::LengthMismatch {
                statement_count,
                owner_count: owners.len(),
            });
        }

        let mut next_ordinal = FxHashMap::<TopLevelOwnerId, usize>::default();
        let statements = owners
            .into_iter()
            .map(|owner| {
                let ordinal = next_ordinal.entry(owner).or_default();
                let owner_local_ordinal = u32::try_from(*ordinal)
                    .map_err(|_| TopLevelOwnerTableError::OwnerLocalOrdinalOverflow { owner })?;
                let statement = TopLevelStatementOwner {
                    owner,
                    owner_local_ordinal,
                };
                *ordinal += 1;
                Ok(statement)
            })
            .collect::<Result<Vec<_>, TopLevelOwnerTableError>>()?;
        Ok(Self {
            statements: statements.into(),
            regions: Arc::from([]),
            fallback_owner: None,
        })
    }

    /// Add explicit, non-overlapping owner regions to a validated statement
    /// table. Regions may be discontiguous for the same owner.
    pub fn try_with_regions<I>(mut self, regions: I) -> Result<Self, TopLevelOwnerRegionError>
    where
        I: IntoIterator<Item = TopLevelOwnerRegion>,
    {
        let mut regions = regions.into_iter().collect::<Vec<_>>();
        regions.sort_unstable_by_key(|region| (region.span.start, region.span.end));
        for (index, region) in regions.iter().enumerate() {
            if region.span.start >= region.span.end {
                return Err(TopLevelOwnerRegionError::EmptyRegion { index });
            }
            if index > 0 && regions[index - 1].span.end > region.span.start {
                return Err(TopLevelOwnerRegionError::OverlappingRegions {
                    previous: index - 1,
                    next: index,
                });
            }
        }
        self.regions = regions.into();
        Ok(self)
    }

    /// Owner table for an ordinary JavaScript/TypeScript file.
    #[must_use]
    pub fn ordinary_file(statement_count: usize) -> Self {
        let mut table = Self::try_from_statement_owners(
            statement_count,
            std::iter::repeat_n(TopLevelOwnerId::ordinary_file(), statement_count),
        )
        .expect("ordinary owner mapping always matches its statement count");
        table.fallback_owner = Some(TopLevelOwnerId::ordinary_file());
        table
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.statements.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    /// Coordinates for a validated program-body statement index.
    #[must_use]
    pub fn statement(&self, statement_index: usize) -> TopLevelStatementOwner {
        self.statements[statement_index]
    }

    #[must_use]
    pub fn statements(&self) -> &[TopLevelStatementOwner] {
        &self.statements
    }

    /// Return the sole validated lexical owner of `kind`.
    ///
    /// Owners are sourced only from this table's validated statement/region
    /// coordinates (plus the explicit ordinary-file fallback). Repeated
    /// coordinates for the same owner are deduplicated; two distinct owners of
    /// the requested kind are ambiguous and fail closed.
    #[must_use]
    pub fn unique_owner_of_kind(&self, kind: TopLevelOwnerKind) -> Option<TopLevelOwnerId> {
        let mut unique = None;
        for owner in self
            .statements
            .iter()
            .map(|statement| statement.owner)
            .chain(self.regions.iter().map(|region| region.owner))
            .chain(self.fallback_owner)
            .filter(|owner| owner.kind() == kind)
        {
            match unique {
                None => unique = Some(owner),
                Some(existing) if existing == owner => {}
                Some(_) => return None,
            }
        }
        unique
    }

    /// Return the sole validated lexical parent visible from `owner`.
    ///
    /// Carrier instance/setup scope sees one unique module owner. Module and
    /// frontmatter scopes never inherit another top-level owner, and multiple
    /// module owners are ambiguous and therefore fail closed.
    #[must_use]
    pub fn validated_lexical_parent_owner(
        &self,
        owner: TopLevelOwnerId,
    ) -> Option<TopLevelOwnerId> {
        (owner.kind() == TopLevelOwnerKind::Instance)
            .then(|| self.unique_owner_of_kind(TopLevelOwnerKind::Module))
            .flatten()
    }

    #[must_use]
    pub fn regions(&self) -> &[TopLevelOwnerRegion] {
        &self.regions
    }

    /// Owner of an explicitly classified source offset. Returns `None` for an
    /// unclassified offset; callers must fail closed rather than guess.
    #[must_use]
    pub fn owner_at_offset(&self, offset: u32) -> Option<TopLevelOwnerId> {
        let index = self
            .regions
            .partition_point(|region| region.span.end <= offset);
        self.regions.get(index).and_then(|region| {
            (region.span.start <= offset && offset < region.span.end).then_some(region.owner)
        })
    }

    /// Owner of an explicit region that fully contains `span`.
    #[must_use]
    pub fn owner_of_span(&self, span: Span) -> Option<TopLevelOwnerId> {
        let owner = self.owner_at_offset(span.start)?;
        let index = self
            .regions
            .partition_point(|region| region.span.end <= span.start);
        self.regions
            .get(index)
            .filter(|region| span.end <= region.span.end)
            .map(|_| owner)
    }

    /// Explicit compatibility owner for authored content that has neither a
    /// parser attachment nor a classified region.
    #[must_use]
    pub fn unattached_fallback_owner(&self) -> Option<TopLevelOwnerId> {
        self.fallback_owner
    }

    /// Resolve a parser-authored comment attachment without inferring an owner
    /// from syntax. Exact statement attachment wins; an unattached comment
    /// uses an explicit owner region; a single-owner ordinary program is the
    /// final unambiguous compatibility case. Ambiguous carrier comments return
    /// `None` and callers fail closed.
    pub fn resolve_comment_owner<I>(
        &self,
        attached_to: u32,
        comment_span: Span,
        statement_starts: I,
    ) -> Option<TopLevelAttachedOwner>
    where
        I: IntoIterator<Item = u32>,
    {
        for (statement_index, statement_start) in statement_starts.into_iter().enumerate() {
            if statement_start == attached_to {
                let statement = self.statement(statement_index);
                return Some(TopLevelAttachedOwner {
                    owner: statement.owner,
                    statement_index: checked_authored_ordinal(statement_index),
                    owner_local_ordinal: Some(statement.owner_local_ordinal),
                });
            }
        }
        if let Some(owner) = self.owner_of_span(comment_span) {
            return Some(TopLevelAttachedOwner {
                owner,
                statement_index: None,
                owner_local_ordinal: None,
            });
        }
        self.fallback_owner.map(|owner| TopLevelAttachedOwner {
            owner,
            statement_index: None,
            owner_local_ordinal: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_partial_or_overlong_mappings() {
        let module = TopLevelOwnerId::module(0);
        assert_eq!(
            TopLevelOwnerTable::try_from_statement_owners(2, [module]),
            Err(TopLevelOwnerTableError::LengthMismatch {
                statement_count: 2,
                owner_count: 1,
            })
        );
        assert_eq!(
            TopLevelOwnerTable::try_from_statement_owners(1, [module, module]),
            Err(TopLevelOwnerTableError::LengthMismatch {
                statement_count: 1,
                owner_count: 2,
            })
        );
        if usize::BITS > u32::BITS {
            let overflow = u32::MAX as usize + 1;
            assert_eq!(
                TopLevelOwnerTable::try_from_statement_owners(overflow, std::iter::empty(),),
                Err(TopLevelOwnerTableError::StatementCountOverflow {
                    statement_count: overflow,
                })
            );
        }
    }

    #[test]
    fn owner_local_ordinals_are_independent_and_stable() {
        let module = TopLevelOwnerId::module(0);
        let instance = TopLevelOwnerId::instance(0);
        let table = TopLevelOwnerTable::try_from_statement_owners(
            5,
            [module, instance, module, instance, module],
        )
        .unwrap();
        let ordinals = table
            .statements()
            .iter()
            .map(|statement| (statement.owner, statement.owner_local_ordinal))
            .collect::<Vec<_>>();
        assert_eq!(
            ordinals,
            vec![
                (module, 0),
                (instance, 0),
                (module, 1),
                (instance, 1),
                (module, 2),
            ]
        );
    }

    #[test]
    fn unique_owner_kind_deduplicates_coordinates_and_rejects_ambiguity() {
        let module = TopLevelOwnerId::module(0);
        let instance = TopLevelOwnerId::instance(0);
        let unique =
            TopLevelOwnerTable::try_from_statement_owners(3, [module, instance, module]).unwrap();
        assert_eq!(
            unique.unique_owner_of_kind(TopLevelOwnerKind::Module),
            Some(module)
        );
        assert_eq!(
            unique.unique_owner_of_kind(TopLevelOwnerKind::Instance),
            Some(instance)
        );

        let ambiguous = TopLevelOwnerTable::try_from_statement_owners(
            3,
            [module, TopLevelOwnerId::module(1), instance],
        )
        .unwrap();
        assert_eq!(
            ambiguous.unique_owner_of_kind(TopLevelOwnerKind::Module),
            None
        );

        assert_eq!(
            unique.validated_lexical_parent_owner(instance),
            Some(module)
        );
        assert_eq!(unique.validated_lexical_parent_owner(module), None);
        assert_eq!(ambiguous.validated_lexical_parent_owner(instance), None);
    }

    #[test]
    fn comment_owner_resolution_prefers_attachment_then_region_and_fails_closed() {
        let module = TopLevelOwnerId::module(0);
        let instance = TopLevelOwnerId::instance(0);
        let table = TopLevelOwnerTable::try_from_statement_owners(2, [module, instance]).unwrap();
        let attached = table
            .resolve_comment_owner(40, Span::new(4, 8), [10, 40])
            .expect("exact statement attachment");
        assert_eq!(attached.owner, instance);
        assert_eq!(attached.owner_local_ordinal, Some(0));
        assert!(table
            .resolve_comment_owner(99, Span::new(4, 8), [10, 40])
            .is_none());

        let with_regions = table
            .try_with_regions([
                TopLevelOwnerRegion {
                    owner: module,
                    span: Span::new(0, 20),
                },
                TopLevelOwnerRegion {
                    owner: instance,
                    span: Span::new(20, 60),
                },
            ])
            .unwrap();
        let regional = with_regions
            .resolve_comment_owner(99, Span::new(30, 35), [10, 40])
            .expect("explicit containing region");
        assert_eq!(regional.owner, instance);
        assert_eq!(regional.owner_local_ordinal, None);
    }

    #[test]
    fn one_owner_carrier_table_has_no_unattached_fallback() {
        let instance = TopLevelOwnerId::instance(0);
        let carrier = TopLevelOwnerTable::try_from_statement_owners(1, [instance]).unwrap();
        assert_eq!(carrier.unattached_fallback_owner(), None);
        assert!(carrier
            .resolve_comment_owner(99, Span::new(0, 1), [10])
            .is_none());

        let ordinary = TopLevelOwnerTable::ordinary_file(1);
        assert_eq!(
            ordinary
                .resolve_comment_owner(99, Span::new(0, 1), [10])
                .expect("ordinary compatibility fallback")
                .owner,
            TopLevelOwnerId::ordinary_file()
        );
    }

    #[test]
    fn authored_ordinal_boundary_rejects_unrepresentable_index() {
        assert_eq!(checked_authored_ordinal(u32::MAX as usize), Some(u32::MAX));
        if usize::BITS > u32::BITS {
            assert_eq!(checked_authored_ordinal(u32::MAX as usize + 1), None);
        }
    }
}
