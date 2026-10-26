//! Fact-aware allocation footprints. The declared conventions and the
//! generic helpers live in [`verter_retention::allocation`], re-exported here.

pub use verter_retention::allocation::*;

use std::sync::Arc;

use verter_retention::heap_size::HeapMeter;

use crate::facts::fact_cache::{FactSet, ReadSetSignature};
use crate::facts::registry::FactKey;
use crate::facts::version::{FactVersionRef, ProgramAnalysisFactRef, ResolveImportsFactRef};

/// Bytes a fact signature keeps alive: its slice block and every fact's
/// owned heap. Evidence receipts carry their own charge (see
/// [`fact_heap_bytes`]).
#[must_use]
pub fn fact_signature_bytes(signature: &ReadSetSignature, aliases: &mut AliasSet) -> usize {
    fact_slice_bytes(&signature.facts, aliases)
}

/// [`fact_signature_bytes`] over a bare fact set.
///
/// A narrow set is charged inline, as every signature was: its slice block
/// and every fact's owned heap. A wide set's tree storage carries its own
/// charge from birth, shared by every set sharing its nodes and claimed by
/// the first retaining admission
/// ([`crate::facts::receipt::reserve_retained_with_evidence`]), so the
/// retaining entry is charged only the set's inline handle here.
#[must_use]
pub fn fact_slice_bytes(facts: &FactSet, aliases: &mut AliasSet) -> usize {
    match facts.flat_arc() {
        Some(flat) => fact_arc_slice_bytes(flat, aliases),
        None => facts
            .tree_storage()
            .filter(|header| aliases.first(header))
            .map_or(0, |_| FactSet::header_bytes()),
    }
}

/// [`fact_slice_bytes`] over a plain shared fact slice.
#[must_use]
pub fn fact_arc_slice_bytes(facts: &Arc<[FactVersionRef]>, aliases: &mut AliasSet) -> usize {
    let Some(block) = arc_slice_block_bytes(facts, aliases) else {
        return 0;
    };
    block + facts_heap_bytes(facts, aliases)
}

/// Bytes an owned fact list keeps alive: its backing by capacity and every
/// fact's owned heap.
#[must_use]
pub fn fact_vec_bytes(facts: &Vec<FactVersionRef>, aliases: &mut AliasSet) -> usize {
    vec_bytes(facts) + facts_heap_bytes(facts, aliases)
}

/// Heap a list of facts owns beyond their inline slots, measured as one
/// value: an allocation several of the facts share is counted once.
#[must_use]
pub fn facts_heap_bytes(facts: &[FactVersionRef], aliases: &mut AliasSet) -> usize {
    FactMeasure::new(aliases).facts(facts)
}

/// Heap one fact owns beyond its inline slot.
///
/// A [`FactVersionRef::Receipt`] costs nothing here: every receipt — a
/// consumed result's evidence — carries its own charge from birth, claimed
/// by the first retaining admission
/// ([`crate::facts::receipt::reserve_retained_with_evidence`]).
///
/// A text with more than one owner is counted once per `aliases`; a text
/// this fact alone owns is counted each time the fact is measured (see
/// [`FactMeasure`]).
#[must_use]
pub fn fact_heap_bytes(fact: &FactVersionRef, aliases: &mut AliasSet) -> usize {
    FactMeasure::new(aliases).fact(fact)
}

/// Where a fact measurement counts the raw allocations facts share: the
/// charge's [`AliasSet`], or a [`HeapMeter`]'s raw set when a fact is
/// part of a measured data tree, so one allocation is offered to one set
/// whichever field of the tree reaches it.
pub trait FactMeter {
    /// `true` the first time this measurement reaches `arc`'s allocation.
    fn first_raw<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool;
    /// Bytes a fact key's leaf name costs this measurement (the declared
    /// leaf-text convention).
    fn leaf_name(&mut self, name: &Arc<str>) -> usize;
    /// The set the owned handles facts reach are offered to.
    fn handles(&mut self) -> &mut AliasSet;
}

impl FactMeter for AliasSet {
    fn first_raw<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        self.first(arc)
    }

    fn leaf_name(&mut self, name: &Arc<str>) -> usize {
        arc_str_bytes(name, self)
    }

    fn handles(&mut self) -> &mut AliasSet {
        self
    }
}

impl FactMeter for HeapMeter<'_, '_> {
    fn first_raw<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        self.first(arc)
    }

    /// Counted like any other raw `Arc<str>` of the tree.
    fn leaf_name(&mut self, name: &Arc<str>) -> usize {
        self.arc_str_bytes(name)
    }

    fn handles(&mut self) -> &mut AliasSet {
        HeapMeter::handles(self)
    }
}

/// One measurement of facts on one [`FactMeter`].
///
/// A fact's canonical id, its resolution key's canonical and specifier
/// texts and a decision's query identity are INLINE allocations: each is
/// charged inside the charge that measures the fact, once per charge
/// (deduplicated on the meter), and never registered as a shared leaf
/// ([`AliasSet::collecting`]). Registering every canonical as a leaf would
/// add a registration, its bookkeeping and a registry insert to every hold
/// for allocations of a few dozen bytes; charging them inline keeps a hold
/// as cheap as when each fact owned its own `String`. A canonical several
/// separately charged carriers share is charged once by each of them: a
/// bounded overcharge of one block per carrier, never an undercharge.
///
/// Only an allocation with more than one owner can be reached twice, so
/// only those are offered to the meter's set: a text this fact alone owns
/// is counted in full without a set entry, and a measurement of facts that
/// share nothing records nothing, exactly as when each owned a `String`.
///
/// The contract is therefore: a shared text is counted once per meter; a
/// single-owner text is counted each time its owner is measured, as the
/// `String` it replaced was. Measuring the same fact twice on one meter
/// counts its single-owner texts twice. No charge does that: a fact is
/// reached only through the value that owns it, every shared container on
/// the way (a signature's `Arc` slice, an owned list's cell, a tree's
/// `Arc`) is itself counted once per meter, and each receipt measures its
/// own facts once on a set of its own.
/// The measurement also remembers the last shared text it counted, so a
/// run of facts naming one allocation offers it once: a canonically sorted
/// list keeps one file's facts of one kind together.
pub struct FactMeasure<'m, M: FactMeter> {
    meter: &'m mut M,
    last_text: usize,
}

impl<'m, M: FactMeter> FactMeasure<'m, M> {
    /// A measurement counting on `meter`.
    pub fn new(meter: &'m mut M) -> Self {
        Self {
            meter,
            last_text: 0,
        }
    }

    /// Heap every fact of `facts` owns, each shared allocation once.
    pub fn facts<'f>(&mut self, facts: impl IntoIterator<Item = &'f FactVersionRef>) -> usize {
        facts.into_iter().map(|fact| self.fact(fact)).sum()
    }

    /// Heap one fact owns beyond its inline slot (see [`fact_heap_bytes`]).
    pub fn fact(&mut self, fact: &FactVersionRef) -> usize {
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, .. }
            | FactVersionRef::DerivedFactHash { canonical_id, .. }
            | FactVersionRef::FileSourceEnv { canonical_id, .. } => self.text(canonical_id),
            FactVersionRef::Parse(parse) => {
                self.text(&parse.canonical_id) + self.fact_key(&parse.key)
            }
            FactVersionRef::RouteSurface(route) => {
                self.text(&route.canonical_id) + self.fact_key(&route.key)
            }
            FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
                canonical_id,
                key,
                ..
            }) => self.text(canonical_id) + self.fact_key(key),
            FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(resolution)) => {
                resolution.key.measure_heap(self)
            }
            FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody {
                function, ..
            }) => {
                let handles = self.meter.handles();
                function.canonical_id.offer(handles)
                    + function.merged_symbol_name.offer(handles)
                    + match &function.function_part {
                        verter_type_expr::facts::FunctionPartIdentity::Member { member_path } => {
                            member_path.offer(handles)
                        }
                        verter_type_expr::facts::FunctionPartIdentity::DeclarationBody
                        | verter_type_expr::facts::FunctionPartIdentity::Initializer
                        | verter_type_expr::facts::FunctionPartIdentity::Other { .. } => 0,
                    }
            }
            FactVersionRef::ProjectGeneration { .. }
            | FactVersionRef::DomainGeneration(_)
            | FactVersionRef::StrictSelfRootWorld(_)
            | FactVersionRef::Receipt(_) => 0,
        }
    }

    /// Bytes an inline text costs this measurement: its block the first
    /// time the measurement reaches it, nothing after, and nothing for the
    /// empty text every `Arc::default()` shares, which allocates nothing.
    pub(crate) fn text(&mut self, text: &Arc<str>) -> usize {
        let address = Arc::as_ptr(text).cast::<u8>() as usize;
        if address == self.last_text {
            return 0;
        }
        self.last_text = address;
        if is_shared_empty_str(text) || !self.first_owned(text) {
            return 0;
        }
        arc_block_bytes(text.len())
    }

    /// `true` the first time this measurement reaches `arc`'s allocation.
    /// An allocation the measured value alone owns cannot be reached again,
    /// so it is counted without a set entry.
    pub(crate) fn first_owned<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        Arc::strong_count(arc) == 1 || self.meter.first_raw(arc)
    }

    /// Heap one fact key's leaf names keep alive.
    fn fact_key(&mut self, key: &FactKey) -> usize {
        let m = &mut *self.meter;
        match key {
            FactKey::Export { name: n, .. } | FactKey::LocalDecl { name: n, .. } => {
                m.leaf_name(&n.0)
            }
            FactKey::ExportAlias { exported_as, .. } => m.leaf_name(&exported_as.0),
            FactKey::Member {
                exporter, name: n, ..
            }
            | FactKey::MemberPresence {
                exporter, name: n, ..
            } => {
                m.leaf_name(&exporter.0)
                    + match n {
                        // An owned name: charged by its own cell, collected here.
                        verter_type_expr::PropertyKey::String(text) => text.offer(m.handles()),
                        _ => 0,
                    }
            }
            FactKey::MemberShape { exporter, .. } => m.leaf_name(&exporter.0),
            FactKey::ImportRef {
                specifier, binding, ..
            } => m.leaf_name(&specifier.0) + m.leaf_name(&binding.0),
            FactKey::SyntacticReexportRef {
                specifier,
                source_name,
                target_name,
                ..
            } => {
                m.leaf_name(&specifier.0)
                    + m.leaf_name(&source_name.0)
                    + m.leaf_name(&target_name.0)
            }
            FactKey::ModuleAugmentation {
                specifier,
                augmented_name,
                ..
            } => m.leaf_name(&specifier.0) + m.leaf_name(&augmented_name.0),
            FactKey::AugmentationContributionSet { specifier, .. }
            | FactKey::AugmentationContributionOrder { specifier, .. } => m.leaf_name(&specifier.0),
            FactKey::DeclContributionOrder { name: n, .. } => m.leaf_name(&n.0),
            FactKey::ResolvedImportClause {
                specifier,
                binding,
                resolved_canonical,
                resolved_source_name,
                ..
            } => {
                m.leaf_name(&specifier.0)
                    + m.leaf_name(&binding.0)
                    + m.leaf_name(resolved_canonical)
                    + m.leaf_name(&resolved_source_name.0)
            }
            FactKey::ResolvedReexportBinding {
                specifier,
                source_name,
                target_name,
                resolved_canonical,
                resolved_source_name,
                ..
            } => {
                m.leaf_name(&specifier.0)
                    + m.leaf_name(&source_name.0)
                    + m.leaf_name(&target_name.0)
                    + m.leaf_name(resolved_canonical)
                    + m.leaf_name(&resolved_source_name.0)
            }
            FactKey::ModuleAugmentationIndexShape {
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
                ..
            } => {
                external_specifier
                    .as_ref()
                    .map_or(0, |specifier| m.leaf_name(&specifier.0))
                    + resolved_relative_canonical
                        .as_ref()
                        .map_or(0, |canonical| m.leaf_name(canonical))
                    + wildcard_pattern
                        .as_ref()
                        .map_or(0, |pattern| m.leaf_name(&pattern.0))
            }
            FactKey::ProgramFileOrder { program } => m.leaf_name(program),
            FactKey::SyntacticExportSet
            | FactKey::SyntacticRouteInterface
            | FactKey::MacroSurface { .. }
            | FactKey::TemplateRoot
            | FactKey::AugmentationTargetSet
            | FactKey::NamespaceScopeSet => 0,
        }
    }
}

impl verter_retention::heap_size::HeapSize for FactVersionRef {
    /// A fact's own heap, its raw allocations counted on the tree's raw
    /// set; a receipt carries its own charge.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        FactMeasure::new(meter).fact(self)
    }
}

impl verter_retention::owned::Measured for FactVersionRef {
    /// A fact's own heap; a receipt carries its own charge.
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        Self::slice_heap_bytes(std::slice::from_ref(self), aliases)
    }

    /// The facts of an owned list, measured as one value. The raw
    /// allocations facts keep are counted with the list that owns them, on
    /// one set of the list's own, so an allocation several facts share is
    /// counted once; their owned names pass to the list's cell.
    fn slice_heap_bytes(items: &[Self], aliases: &mut AliasSet) -> usize {
        let mut raw = AliasSet::new();
        let bytes = facts_heap_bytes(items, &mut raw);
        bytes + aliases.absorb(&mut raw)
    }
}

impl verter_retention::heap_size::HeapSize for crate::facts::fact_cache::ReadSetSignature {
    /// The fact slice and every fact's own heap, on the tree's raw set.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        // A tree set's nodes charge themselves; its header is this value's.
        match self.facts.flat_arc() {
            Some(flat) => flat.heap_size(meter),
            None => self
                .facts
                .tree_storage()
                .filter(|header| meter.first(header))
                .map_or(0, |_| FactSet::header_bytes()),
        }
    }
}

#[cfg(test)]
#[path = "allocation_tests.rs"]
mod tests;
