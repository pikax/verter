//! Allocation footprints: the bytes a retained value actually keeps alive.
//!
//! A charge follows the ALLOCATIONS its owner keeps reachable — never a
//! fixed per-item proxy. A fact naming a ten-kilobyte canonical path costs
//! ten kilobytes, a diagnostic carrying a long name costs that name, and a
//! slice costs its whole backing block. The helpers here measure one
//! allocation each; the semantic owners compose them over their payloads.
//!
//! # Declared conventions
//!
//! * **Heap block.** One heap allocation of `n > 0` payload bytes costs `n`
//!   rounded up to [`ALLOCATION_GRANULE_BYTES`], plus
//!   [`ALLOCATION_HEADER_BYTES`] of allocator bookkeeping
//!   ([`heap_block_bytes`]). A zero-byte payload allocates nothing. These
//!   are the physical-policy allocator-overhead conventions: a real
//!   allocator's size classes differ, but the convention is the same for
//!   every owner, so no owner is charged as free and none twice.
//! * **Capacity, not length.** A `String` or `Vec` costs its capacity.
//! * **`Arc` blocks.** An `Arc<T>` block holds its two reference counters
//!   beside `T`; an `Arc<[T]>` / `Arc<str>` block holds them beside the
//!   whole slice, so even an empty `Arc` slice costs its counters.
//! * **Embedding.** A field's INLINE bytes live inside the block of the
//!   value that embeds it and are charged with that block; only the heap a
//!   field points at is charged on its own. A value held directly in a
//!   container slot is charged by the container's slot convention.
//! * **Self-charged allocations.** An allocation that carries its own
//!   charge — an evidence page, a cost receipt, a pooled identity string, a
//!   graph node — is never charged again by a holder: holding it costs the
//!   holder only the handle's inline bytes.
//! * **Aliases inside one charge.** Within one charge each distinct `Arc`
//!   allocation is counted once ([`AliasSet`]). Allocations several
//!   independently charged owners share are charged once process-wide
//!   through [`super::shared::AliasCharges`].
//! * **Leaf text.** A leaf `Arc<str>` name inside a registered carrier (a
//!   fact's interned names, a dependency's canonical, a diagnostic's name)
//!   is its own shared allocation: measuring a carrier for registration
//!   collects its leaves instead of counting them ([`AliasSet::collecting`])
//!   and the registry charges each leaf once, held by every carrier that
//!   reaches it ([`super::shared::AliasCharges`]). Outside registration a
//!   measurement counts each distinct leaf it reaches in full.
//!
//! The figures are admission ESTIMATES (see the module docs of
//! [`super`]): nothing in cache validity reads them.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::facts::fact_cache::ReadSetSignature;
use crate::facts::registry::FactKey;
use crate::facts::resolution::ResolutionFactKey;
use crate::facts::version::{FactVersionRef, ProgramAnalysisFactRef, ResolveImportsFactRef};

/// Allocator bookkeeping one heap block costs beside its payload.
pub const ALLOCATION_HEADER_BYTES: usize = 16;
/// Allocator size-class granularity a heap block's payload rounds up to.
pub const ALLOCATION_GRANULE_BYTES: usize = 16;

/// Bytes one heap block holding `payload` bytes costs.
#[must_use]
pub const fn heap_block_bytes(payload: usize) -> usize {
    if payload == 0 {
        return 0;
    }
    payload.div_ceil(ALLOCATION_GRANULE_BYTES) * ALLOCATION_GRANULE_BYTES + ALLOCATION_HEADER_BYTES
}

/// Bytes one `Arc` block holding `payload` bytes beside its two counters
/// costs.
#[must_use]
pub const fn arc_block_bytes(payload: usize) -> usize {
    heap_block_bytes(2 * std::mem::size_of::<usize>() + payload)
}

/// Bytes a `String`'s backing costs: its capacity.
#[must_use]
pub fn string_bytes(value: &String) -> usize {
    heap_block_bytes(value.capacity())
}

/// Bytes a `Vec`'s backing costs: its capacity, not its length.
#[must_use]
pub fn vec_bytes<T>(value: &Vec<T>) -> usize {
    heap_block_bytes(value.capacity() * std::mem::size_of::<T>())
}

/// Bytes a boxed `T` costs.
#[must_use]
pub const fn box_bytes<T>() -> usize {
    heap_block_bytes(std::mem::size_of::<T>())
}

/// Distinct `Arc` allocations one charge has already counted.
///
/// Keyed by the allocation's address. Every key names an allocation the
/// measured value holds for the whole measurement, so an address cannot be
/// freed and reused while the set is live. The first few keys stay inline,
/// so measuring a small value allocates nothing.
#[derive(Default)]
pub struct AliasSet {
    inline: smallvec::SmallVec<[usize; ALIAS_INLINE]>,
    spilled: Option<FxHashSet<usize>>,
    /// When collecting, the distinct leaf names the measurement reached,
    /// which it then does not count.
    leaves: Option<Vec<Arc<str>>>,
    /// When collecting, the distinct owned allocations the measurement
    /// reached, each charged by its own cell and not counted here.
    children: Option<Vec<Arc<dyn super::owned::Node>>>,
}

/// Allocations an [`AliasSet`] tracks before it spills to a hash set.
const ALIAS_INLINE: usize = 8;

impl AliasSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty set that collects every distinct leaf name a measurement
    /// reaches instead of counting it: a registered carrier's leaves are
    /// charged as allocations of their own.
    #[must_use]
    pub fn collecting() -> Self {
        Self {
            leaves: Some(Vec::new()),
            children: Some(Vec::new()),
            ..Self::default()
        }
    }

    /// Record an owned child when collecting; `false` when this
    /// measurement counts children in full instead.
    pub(crate) fn collect_child(
        &mut self,
        child: impl FnOnce() -> Arc<dyn super::owned::Node>,
    ) -> bool {
        match self.children.as_mut() {
            Some(children) => {
                children.push(child());
                true
            }
            None => false,
        }
    }

    /// The owned children a collecting measurement reached, each once.
    pub(crate) fn take_children(&mut self) -> Box<[Arc<dyn super::owned::Node>]> {
        self.children.take().unwrap_or_default().into_boxed_slice()
    }

    /// The leaf names a collecting measurement reached, each once.
    #[must_use]
    pub fn take_leaves(&mut self) -> Vec<Arc<str>> {
        self.leaves.take().unwrap_or_default()
    }

    /// `true` the first time `arc`'s allocation is offered, `false` after.
    pub fn first<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        let key = Arc::as_ptr(arc).cast::<()>() as usize;
        if let Some(spilled) = self.spilled.as_mut() {
            return spilled.insert(key);
        }
        if self.inline.contains(&key) {
            return false;
        }
        if self.inline.len() < ALIAS_INLINE {
            self.inline.push(key);
            return true;
        }
        let mut spilled: FxHashSet<usize> = self.inline.drain(..).collect();
        spilled.insert(key);
        self.spilled = Some(spilled);
        true
    }

    /// Distinct allocations counted so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.spilled
            .as_ref()
            .map_or(self.inline.len(), FxHashSet::len)
    }

    /// Whether nothing has been counted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Bytes an `Arc<str>` keeps alive, or `0` when this charge already counted
/// its allocation.
#[must_use]
pub fn arc_str_bytes(value: &Arc<str>, aliases: &mut AliasSet) -> usize {
    if !aliases.first(value) {
        return 0;
    }
    match aliases.leaves.as_mut() {
        Some(leaves) => {
            leaves.push(Arc::clone(value));
            0
        }
        None => arc_block_bytes(value.len()),
    }
}

/// Bytes an `Arc<[T]>`'s own block keeps alive — the slice's inline
/// elements and the counters — or `None` when this charge already counted
/// the allocation (its elements' heap then counted with it).
#[must_use]
pub fn arc_slice_block_bytes<T>(value: &Arc<[T]>, aliases: &mut AliasSet) -> Option<usize> {
    aliases
        .first(value)
        .then(|| arc_block_bytes(std::mem::size_of_val::<[T]>(value)))
}

/// Bytes a list of leaf names keeps alive: the list's block and each
/// distinct name's text.
#[must_use]
pub fn arc_str_list_bytes(value: &Arc<[Arc<str>]>, aliases: &mut AliasSet) -> usize {
    let Some(block) = arc_slice_block_bytes(value, aliases) else {
        return 0;
    };
    block
        + value
            .iter()
            .map(|name| arc_str_bytes(name, aliases))
            .sum::<usize>()
}

/// Bytes a fact signature keeps alive: its slice block and every fact's
/// owned heap. Evidence receipts carry their own charge (see
/// [`fact_heap_bytes`]).
#[must_use]
pub fn fact_signature_bytes(signature: &ReadSetSignature, aliases: &mut AliasSet) -> usize {
    fact_slice_bytes(&signature.facts, aliases)
}

/// [`fact_signature_bytes`] over a bare fact slice.
#[must_use]
pub fn fact_slice_bytes(facts: &Arc<[FactVersionRef]>, aliases: &mut AliasSet) -> usize {
    let Some(block) = arc_slice_block_bytes(facts, aliases) else {
        return 0;
    };
    block
        + facts
            .iter()
            .map(|fact| fact_heap_bytes(fact, aliases))
            .sum::<usize>()
}

/// Bytes an owned fact list keeps alive: its backing by capacity and every
/// fact's owned heap.
#[must_use]
pub fn fact_vec_bytes(facts: &Vec<FactVersionRef>, aliases: &mut AliasSet) -> usize {
    vec_bytes(facts)
        + facts
            .iter()
            .map(|fact| fact_heap_bytes(fact, aliases))
            .sum::<usize>()
}

/// Heap one fact owns beyond its inline slot.
///
/// A [`FactVersionRef::Receipt`] costs nothing here: every receipt — a
/// result's evidence or a page — carries its own charge from birth, claimed
/// by the first retaining admission
/// ([`crate::facts::receipt::reserve_retained_with_evidence`]).
#[must_use]
pub fn fact_heap_bytes(fact: &FactVersionRef, aliases: &mut AliasSet) -> usize {
    match fact {
        FactVersionRef::FileWholeHash { canonical_id, .. }
        | FactVersionRef::DerivedFactHash { canonical_id, .. }
        | FactVersionRef::FileSourceEnv { canonical_id, .. } => string_bytes(canonical_id),
        FactVersionRef::Parse(parse) => {
            string_bytes(&parse.canonical_id) + fact_key_heap_bytes(&parse.key, aliases)
        }
        FactVersionRef::RouteSurface(route) => {
            string_bytes(&route.canonical_id) + fact_key_heap_bytes(&route.key, aliases)
        }
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
            canonical_id,
            key,
            ..
        }) => string_bytes(canonical_id) + fact_key_heap_bytes(key, aliases),
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(resolution)) => {
            resolution_key_heap_bytes(&resolution.key)
        }
        FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody { function, .. }) => {
            arc_str_bytes(&function.canonical_id, aliases)
                + arc_str_bytes(&function.merged_symbol_name, aliases)
                + match &function.function_part {
                    verter_type_expr::facts::FunctionPartIdentity::Member { member_path } => {
                        arc_slice_block_bytes(member_path, aliases).unwrap_or(0)
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

/// Heap one fact key's leaf names keep alive.
fn fact_key_heap_bytes(key: &FactKey, aliases: &mut AliasSet) -> usize {
    let mut bytes = 0;
    let mut name = |text: &Arc<str>| bytes += arc_str_bytes(text, aliases);
    match key {
        FactKey::Export { name: n, .. } | FactKey::LocalDecl { name: n, .. } => name(&n.0),
        FactKey::ExportAlias { exported_as, .. } => name(&exported_as.0),
        FactKey::Member {
            exporter, name: n, ..
        }
        | FactKey::MemberPresence {
            exporter, name: n, ..
        } => {
            name(&exporter.0);
            if let verter_type_expr::PropertyKey::String(text) = n {
                name(text);
            }
        }
        FactKey::MemberShape { exporter, .. } => name(&exporter.0),
        FactKey::ImportRef {
            specifier, binding, ..
        } => {
            name(&specifier.0);
            name(&binding.0);
        }
        FactKey::SyntacticReexportRef {
            specifier,
            source_name,
            target_name,
            ..
        } => {
            name(&specifier.0);
            name(&source_name.0);
            name(&target_name.0);
        }
        FactKey::ModuleAugmentation {
            specifier,
            augmented_name,
            ..
        } => {
            name(&specifier.0);
            name(&augmented_name.0);
        }
        FactKey::AugmentationContributionSet { specifier, .. }
        | FactKey::AugmentationContributionOrder { specifier, .. } => name(&specifier.0),
        FactKey::DeclContributionOrder { name: n, .. } => name(&n.0),
        FactKey::ResolvedImportClause {
            specifier,
            binding,
            resolved_canonical,
            resolved_source_name,
            ..
        } => {
            name(&specifier.0);
            name(&binding.0);
            name(resolved_canonical);
            name(&resolved_source_name.0);
        }
        FactKey::ResolvedReexportBinding {
            specifier,
            source_name,
            target_name,
            resolved_canonical,
            resolved_source_name,
            ..
        } => {
            name(&specifier.0);
            name(&source_name.0);
            name(&target_name.0);
            name(resolved_canonical);
            name(&resolved_source_name.0);
        }
        FactKey::ModuleAugmentationIndexShape {
            external_specifier,
            resolved_relative_canonical,
            wildcard_pattern,
            ..
        } => {
            if let Some(specifier) = external_specifier {
                name(&specifier.0);
            }
            if let Some(canonical) = resolved_relative_canonical {
                name(canonical);
            }
            if let Some(pattern) = wildcard_pattern {
                name(&pattern.0);
            }
        }
        FactKey::ProgramFileOrder { program } => name(program),
        FactKey::SyntacticExportSet
        | FactKey::SyntacticRouteInterface
        | FactKey::MacroSurface { .. }
        | FactKey::TemplateRoot
        | FactKey::AugmentationTargetSet
        | FactKey::NamespaceScopeSet => {}
    }
    bytes
}

/// Heap one resolution fact key owns: its canonical and specifier strings,
/// and a decision's boxed query identity.
fn resolution_key_heap_bytes(key: &ResolutionFactKey) -> usize {
    key.owned_heap_bytes()
}

#[cfg(test)]
#[path = "allocation_tests.rs"]
mod tests;
