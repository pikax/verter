//! The fact contract between index consumers and the host's producers.
//!
//! A consumer demands the surfaces it reads, and each is certified on its own:
//! component auto-import demands the component inventory without walking a
//! template, global-class definition demands stylesheet definitions, and
//! references additionally demand markup class occurrences. A producer answers
//! a demanded surface for one file with the facts and their coverage together,
//! so a slot is certified only for the exact value its projector consumed.
//! Nothing here reads [`verter_semantic::analysis::AnalysisScope`] as a proof:
//! the scope is the policy for eager work, and a demand produces what it needs.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

use super::identity::{PartitionFamily, PartitionId, ScopeId};
use super::query::DeclaredScope;

/// One certified fact surface the host projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndexSurface {
    /// The admitted carrier identity of every component file. It needs no
    /// template.
    ComponentInventory,
    /// Template component occurrences.
    ComponentUses,
    /// Stylesheet classes and IDs with their scope.
    StyleDefinitions,
    /// Class tokens in markup: Vue template `class` attributes or Svelte
    /// markup class tokens.
    MarkupClassReferences,
    /// CSS custom property flow: stylesheet definitions and `var()`
    /// references, inline-style facts in markup, and script manipulations.
    CssVariables,
    /// Script declarations and imported local names.
    Bindings,
    /// Authored import relations.
    Imports,
    /// Owner-qualified domain registrations: provide/inject keys, store
    /// definitions and uses, and declared or listened events.
    Registrations,
}

impl IndexSurface {
    /// Every surface, in certification order.
    pub const ALL: [Self; 8] = [
        Self::ComponentInventory,
        Self::ComponentUses,
        Self::StyleDefinitions,
        Self::MarkupClassReferences,
        Self::CssVariables,
        Self::Bindings,
        Self::Imports,
        Self::Registrations,
    ];

    /// The partition whose postings this surface's rows answer under.
    #[must_use]
    pub fn partition(self) -> IndexDemandPartition {
        match self {
            Self::ComponentInventory | Self::ComponentUses => IndexDemandPartition::Components,
            Self::StyleDefinitions | Self::MarkupClassReferences => {
                IndexDemandPartition::StyleSelectors
            }
            Self::CssVariables => IndexDemandPartition::CssVariables,
            Self::Bindings => IndexDemandPartition::Bindings,
            Self::Imports => IndexDemandPartition::Imports,
            Self::Registrations => IndexDemandPartition::Registrations,
        }
    }

    /// The fact family this surface posts under.
    #[must_use]
    pub fn family(self) -> PartitionFamily {
        self.partition().family()
    }

    /// Stable facet discriminant inside the family, independent of
    /// declaration order. Never zero: zero is the undivided family.
    pub(super) const fn facet(self) -> u32 {
        match self {
            Self::ComponentInventory => 1,
            Self::ComponentUses => 2,
            Self::StyleDefinitions => 3,
            Self::MarkupClassReferences => 4,
            Self::CssVariables => 5,
            Self::Bindings => 6,
            Self::Imports => 7,
            Self::Registrations => 8,
        }
    }

    /// The host's partition of this surface.
    #[must_use]
    pub fn id(self) -> PartitionId {
        PartitionId::faceted(
            super::host_projection::producer().clone(),
            self.family(),
            self.facet(),
        )
    }
}

/// A fact family the host can project. Demanding one demands every surface
/// of it; demand the [`IndexSurface`] instead to pay for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndexDemandPartition {
    /// CSS custom property definitions, `var()` references, template style
    /// bindings and script manipulations.
    CssVariables,
    /// Stylesheet classes and IDs, and the class tokens markup references.
    StyleSelectors,
    /// Script declarations and imported local names.
    Bindings,
    /// Component views and template component uses.
    Components,
    /// Authored import relations.
    Imports,
    /// Domain registrations.
    Registrations,
}

impl IndexDemandPartition {
    /// The fact family this demand reads.
    #[must_use]
    pub fn family(self) -> PartitionFamily {
        match self {
            Self::CssVariables => PartitionFamily::CssVariables,
            Self::StyleSelectors => PartitionFamily::StyleSelectors,
            Self::Bindings => PartitionFamily::Bindings,
            Self::Components => PartitionFamily::Components,
            Self::Imports => PartitionFamily::Imports,
            Self::Registrations => PartitionFamily::Registrations,
        }
    }

    /// The surfaces this partition is made of.
    #[must_use]
    pub fn surfaces(self) -> &'static [IndexSurface] {
        match self {
            Self::CssVariables => &[IndexSurface::CssVariables],
            Self::StyleSelectors => &[
                IndexSurface::StyleDefinitions,
                IndexSurface::MarkupClassReferences,
            ],
            Self::Bindings => &[IndexSurface::Bindings],
            Self::Components => &[
                IndexSurface::ComponentInventory,
                IndexSurface::ComponentUses,
            ],
            Self::Imports => &[IndexSurface::Imports],
            Self::Registrations => &[IndexSurface::Registrations],
        }
    }
}

/// What an [`IndexDemand`] can be built from: a surface, or a partition
/// standing for all of its surfaces.
pub trait DemandItem {
    /// The surfaces this item demands.
    fn demanded_surfaces(self) -> &'static [IndexSurface];
}

impl DemandItem for IndexSurface {
    fn demanded_surfaces(self) -> &'static [IndexSurface] {
        let index = IndexSurface::ALL
            .iter()
            .position(|surface| *surface == self)
            .unwrap_or(0);
        &IndexSurface::ALL[index..=index]
    }
}

impl DemandItem for IndexDemandPartition {
    fn demanded_surfaces(self) -> &'static [IndexSurface] {
        self.surfaces()
    }
}

/// One explicit request for index facts. Undemanded surfaces run nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDemand {
    surfaces: BTreeSet<IndexSurface>,
}

impl IndexDemand {
    /// Demand these surfaces, or every surface of these partitions.
    #[must_use]
    pub fn of<I>(items: I) -> Self
    where
        I: IntoIterator,
        I::Item: DemandItem,
    {
        Self {
            surfaces: items
                .into_iter()
                .flat_map(|item| item.demanded_surfaces().iter().copied())
                .collect(),
        }
    }

    /// The demanded surfaces.
    #[must_use]
    pub fn surfaces(&self) -> &BTreeSet<IndexSurface> {
        &self.surfaces
    }

    /// The partitions the demanded surfaces post under.
    #[must_use]
    pub fn partitions(&self) -> BTreeSet<IndexDemandPartition> {
        self.surfaces
            .iter()
            .map(|surface| surface.partition())
            .collect()
    }

    /// The population a complete walk of this demand certifies.
    #[must_use]
    pub fn scope(&self) -> ScopeId {
        ScopeId::of_population(self)
    }

    /// The declared population a query of this demand reads. A key of any
    /// other partition is outside it.
    #[must_use]
    pub fn declared_scope(&self) -> DeclaredScope {
        DeclaredScope::new(self.scope(), self.partition_ids())
    }

    pub(super) fn partition_ids(&self) -> BTreeSet<PartitionId> {
        self.surfaces.iter().map(|surface| surface.id()).collect()
    }
}

impl CanonicalEncode for IndexDemand {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.host_scope.v2";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_u32(1, 1);
        for surface in &self.surfaces {
            encoder.field_u32(2, surface.facet());
        }
    }
}

/// How far a producer answered one surface for one file. Only the first two
/// certify a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactCoverage {
    /// Every fact of this surface was produced for the current source
    /// revision.
    Complete,
    /// The file legitimately has no facts of this surface: no style block, no
    /// template, no markup.
    Absent,
    /// The producer could not produce the surface now: a missing owner
    /// artifact, content it cannot reach, a failed admission. A later request
    /// may succeed.
    Unavailable { reason: Arc<str> },
    /// No producer exists for this surface of this file under this host.
    Unsupported { reason: Arc<str> },
}

impl FactCoverage {
    /// Whether a slot projected from this coverage may be certified.
    #[must_use]
    pub fn certifies(&self) -> bool {
        matches!(self, Self::Complete | Self::Absent)
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Absent => 0,
            Self::Complete => 1,
            Self::Unsupported { .. } => 2,
            Self::Unavailable { .. } => 3,
        }
    }

    /// The coverage of a surface read from several producers: the weakest
    /// answer wins, and present facts outrank absent ones.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    pub(super) fn unavailable(reason: &'static str) -> Self {
        Self::Unavailable {
            reason: Arc::from(reason),
        }
    }

    pub(super) fn unsupported(reason: &'static str) -> Self {
        Self::Unsupported {
            reason: Arc::from(reason),
        }
    }
}

/// A producer's answer for one surface of one file: the facts it produced,
/// and how far they cover the surface. A projector consumes `value` and
/// certifies `coverage`, never one without the other.
#[derive(Debug, Clone)]
pub struct FactReceipt<T> {
    pub value: Option<T>,
    pub coverage: FactCoverage,
}

impl<T> FactReceipt<T> {
    pub(super) fn complete(value: T) -> Self {
        Self {
            value: Some(value),
            coverage: FactCoverage::Complete,
        }
    }

    pub(super) fn absent() -> Self {
        Self {
            value: None,
            coverage: FactCoverage::Absent,
        }
    }

    pub(super) fn without_value(coverage: FactCoverage) -> Self {
        Self {
            value: None,
            coverage,
        }
    }
}
