//! Exact correspondence between frame-local binding IDs and indexed declarations.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{FrameSpan, FunctionBodySkeleton, SkeletonBindingId, SkeletonBindingKind};
use crate::analysis::function_program::{
    FlowBindingIdentity, FunctionBindingKind, FunctionBindingRecord, FunctionProgramKey,
};

/// A resolved value reference. Names are display metadata, never lookup keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub enum FlowBindingRef {
    Local(SkeletonBindingId),
    Captured(FlowBindingIdentity),
}

/// An inventory and skeleton that cannot describe the same declaration universe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowBindingMapError {
    DuplicateDeclaration,
    MissingDeclaration,
    ExtraDeclaration,
    MissingOccurrence,
    UnmodeledOccurrence,
    ConflictingOccurrence,
    InvalidSpan,
    TooManyBindings,
}

/// A validated bijection over all value-bearing declarations of one function.
/// Type-only skeleton bindings intentionally have no value identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowBindingMap {
    function: FunctionProgramKey,
    identities: Arc<[Option<FlowBindingIdentity>]>,
    locals: Arc<[SkeletonBindingId]>,
    runtime_locals: Arc<[SkeletonBindingId]>,
    runtime_declarations: Arc<[SkeletonBindingId]>,
    declaration_offsets: Arc<[u32]>,
    runtime_shapes: Arc<[FlowRuntimeBindingShape]>,
    occurrences: Arc<FxHashMap<FrameSpan, IndexedOccurrence>>,
    source_type_queries: Arc<FxHashMap<FrameSpan, IndexedOccurrence>>,
    declarations_by_span: Arc<FxHashMap<DeclarationSpan, SkeletonBindingId>>,
}

/// An exact declaration address. Equality is also the actual lookup-work probe.
#[derive(Debug, Clone, Copy)]
struct DeclarationSpan(FrameSpan);

impl PartialEq for DeclarationSpan {
    fn eq(&self, other: &Self) -> bool {
        #[cfg(test)]
        record_declaration_span_comparison();
        self.0 == other.0
    }
}

impl Eq for DeclarationSpan {}

impl std::hash::Hash for DeclarationSpan {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.0, state);
    }
}

/// Constant-size structural facts for one canonical runtime declaration group.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FlowRuntimeBindingShape {
    pub has_var: bool,
    pub has_destructured_var: bool,
}

/// The indexed authority for one exact authored identifier occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowBindingOccurrence<'a> {
    Resolved(&'a FlowBindingRef),
    UnmodeledLocal,
    Free,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IndexedOccurrence {
    Resolved(FlowBindingRef),
    UnmodeledLocal,
    Free,
}

fn indexed_occurrence(occurrence: Option<&IndexedOccurrence>) -> FlowBindingOccurrence<'_> {
    match occurrence {
        Some(IndexedOccurrence::Resolved(binding)) => FlowBindingOccurrence::Resolved(binding),
        Some(IndexedOccurrence::UnmodeledLocal) => FlowBindingOccurrence::UnmodeledLocal,
        Some(IndexedOccurrence::Free) => FlowBindingOccurrence::Free,
        None => FlowBindingOccurrence::Missing,
    }
}

impl FlowBindingMap {
    pub fn build(
        skeleton: &FunctionBodySkeleton,
        inventory: &[FunctionBindingRecord],
        function: &FunctionProgramKey,
        anchor: u32,
    ) -> Result<Self, FlowBindingMapError> {
        let mut declarations = FxHashMap::default();
        for (slot, record) in inventory.iter().enumerate() {
            if record.span.start < anchor || record.span.end <= record.span.start {
                return Err(FlowBindingMapError::InvalidSpan);
            }
            let slot = u32::try_from(slot).map_err(|_| FlowBindingMapError::TooManyBindings)?;
            if declarations
                .insert((FrameSpan::rebase(anchor, record.span), record.kind), slot)
                .is_some()
            {
                return Err(FlowBindingMapError::DuplicateDeclaration);
            }
        }
        let mut declarations_by_span =
            FxHashMap::with_capacity_and_hasher(skeleton.bindings.len(), Default::default());
        let mut identities = Vec::with_capacity(skeleton.bindings.len());
        let mut locals = vec![None; inventory.len()];
        for (ordinal, binding) in skeleton.bindings.iter().enumerate() {
            let ordinal =
                u32::try_from(ordinal).map_err(|_| FlowBindingMapError::TooManyBindings)?;
            if declarations_by_span
                .insert(
                    DeclarationSpan(binding.span),
                    SkeletonBindingId::from_index(ordinal),
                )
                .is_some()
            {
                return Err(FlowBindingMapError::DuplicateDeclaration);
            }
            let Some(kind) = binding.kind.function_binding_kind() else {
                identities.push(None);
                continue;
            };
            let slot = declarations
                .remove(&(binding.span, kind))
                .ok_or(FlowBindingMapError::MissingDeclaration)?;
            let record = &inventory[slot as usize];
            locals[slot as usize] = Some(SkeletonBindingId::from_index(ordinal));
            identities.push(Some(FlowBindingIdentity {
                name: Arc::clone(&record.name),
                kind,
                defining_function: function.clone(),
                binding_slot: slot,
            }));
        }
        if !declarations.is_empty() {
            return Err(FlowBindingMapError::ExtraDeclaration);
        }
        let locals = locals
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(FlowBindingMapError::ExtraDeclaration)?;
        let canonical_slots =
            crate::analysis::function_program::canonical_runtime_binding_slots(inventory);
        let runtime_locals: Vec<_> = identities
            .iter()
            .enumerate()
            .map(|(ordinal, identity)| {
                identity.as_ref().map_or_else(
                    || SkeletonBindingId::from_index(ordinal as u32),
                    |identity| locals[canonical_slots[identity.binding_slot as usize] as usize],
                )
            })
            .collect();
        let mut runtime_shapes = vec![FlowRuntimeBindingShape::default(); skeleton.bindings.len()];
        let mut offsets = vec![0_u32; skeleton.bindings.len() + 1];
        for (ordinal, runtime) in runtime_locals.iter().enumerate() {
            if identities[ordinal].is_some() {
                let binding = &skeleton.bindings[ordinal];
                if binding.kind == SkeletonBindingKind::Var {
                    runtime_shapes[runtime.index()].has_var = true;
                    runtime_shapes[runtime.index()].has_destructured_var |= binding.destructured;
                }
                offsets[runtime.index() + 1] += 1;
            }
        }
        for ordinal in 1..offsets.len() {
            offsets[ordinal] += offsets[ordinal - 1];
        }
        let mut positions = offsets.clone();
        let mut groups = vec![SkeletonBindingId::from_index(0); inventory.len()];
        for (ordinal, runtime) in runtime_locals.iter().enumerate() {
            if identities[ordinal].is_some() {
                let position = &mut positions[runtime.index()];
                groups[*position as usize] = SkeletonBindingId::from_index(ordinal as u32);
                *position += 1;
            }
        }
        Ok(Self {
            function: function.clone(),
            identities: identities.into(),
            locals: locals.into(),
            runtime_locals: runtime_locals.into(),
            runtime_declarations: groups.into(),
            declaration_offsets: offsets.into(),
            runtime_shapes: runtime_shapes.into(),
            occurrences: Arc::new(FxHashMap::default()),
            source_type_queries: Arc::new(FxHashMap::default()),
            declarations_by_span: Arc::new(declarations_by_span),
        })
    }

    /// Resolve an authored declaration identifier without scanning the frame.
    /// This preserves its exact declaration ID, including type-only declarations;
    /// runtime storage aliases are obtained separately through `canonical_local`.
    pub fn declaration_at_span(&self, span: FrameSpan) -> Option<SkeletonBindingId> {
        self.declarations_by_span
            .get(&DeclarationSpan(span))
            .copied()
    }

    pub(super) fn resolve_identity(
        &self,
        identity: &FlowBindingIdentity,
    ) -> Result<FlowBindingRef, FlowBindingMapError> {
        if identity.defining_function == self.function {
            let local = self
                .local(identity)
                .ok_or(FlowBindingMapError::MissingDeclaration)?;
            Ok(FlowBindingRef::Local(self.canonical_local(local)))
        } else {
            Ok(FlowBindingRef::Captured(identity.clone()))
        }
    }

    pub(super) fn prepare_occurrences(
        &mut self,
        entry: &crate::analysis::function_program::FunctionProgramEntry,
    ) -> Result<(), FlowBindingMapError> {
        use crate::analysis::function_program::FunctionWriteTarget;
        let mut occurrences = FxHashMap::default();
        let mut insert = |span: verter_span::Span,
                          binding: IndexedOccurrence|
         -> Result<(), FlowBindingMapError> {
            if span.start < entry.span.start || span.end > entry.span.end || span.start >= span.end
            {
                return Err(FlowBindingMapError::InvalidSpan);
            }
            let span = FrameSpan::rebase(entry.span.start, span);
            if let Some(previous) = occurrences.insert(span, binding.clone()) {
                if previous != binding {
                    return Err(FlowBindingMapError::ConflictingOccurrence);
                }
            }
            Ok(())
        };
        for (slot, declaration) in entry.bindings.iter().enumerate() {
            insert(
                declaration.span,
                IndexedOccurrence::Resolved(FlowBindingRef::Local(
                    self.canonical_local(self.locals[slot]),
                )),
            )?;
        }
        for declaration in entry.unmodeled_bindings.iter() {
            insert(declaration.span, IndexedOccurrence::UnmodeledLocal)?;
        }
        let reference_binding = |binding: &crate::analysis::function_program::FunctionReferenceBinding| -> Result<IndexedOccurrence, FlowBindingMapError> {
            use crate::analysis::function_program::FunctionReferenceBinding;
            Ok(match binding {
                FunctionReferenceBinding::Resolved(identity) => IndexedOccurrence::Resolved(self.resolve_identity(identity)?),
                FunctionReferenceBinding::Free => IndexedOccurrence::Free,
                FunctionReferenceBinding::UnmodeledLocal => IndexedOccurrence::UnmodeledLocal,
            })
        };
        for reference in entry.references.iter() {
            insert(reference.span, reference_binding(&reference.binding)?)?;
        }
        for target in entry.writes.iter().flat_map(|write| write.targets.iter()) {
            if let FunctionWriteTarget::Binding { reference, .. } = target {
                insert(reference.span, reference_binding(&reference.binding)?)?;
            }
        }
        let mut queries = FxHashMap::default();
        for query in entry.source_type_queries.iter() {
            if query.span.start < entry.span.start
                || query.span.end > entry.span.end
                || query.span.start >= query.span.end
            {
                return Err(FlowBindingMapError::InvalidSpan);
            }
            let span = FrameSpan::rebase(entry.span.start, query.span);
            let binding = reference_binding(&query.binding)?;
            if queries
                .insert(span, binding.clone())
                .is_some_and(|previous| previous != binding)
            {
                return Err(FlowBindingMapError::ConflictingOccurrence);
            }
        }
        self.occurrences = Arc::new(occurrences);
        self.source_type_queries = Arc::new(queries);
        Ok(())
    }

    /// Exact read/write/callee occurrence authority. Known free and absent
    /// occurrences are distinct; neither permits a runtime name fallback.
    pub fn occurrence(&self, span: FrameSpan) -> FlowBindingOccurrence<'_> {
        indexed_occurrence(self.occurrences.get(&span))
    }

    /// Exact lexical authority for authored whole type queries. Type queries
    /// never enter the runtime occurrence or capture inventory.
    pub fn source_type_query_occurrence(&self, span: FrameSpan) -> FlowBindingOccurrence<'_> {
        indexed_occurrence(self.source_type_queries.get(&span))
    }

    pub(super) fn required_source_type_query(
        &self,
        span: FrameSpan,
    ) -> Result<Option<FlowBindingRef>, FlowBindingMapError> {
        match self.source_type_query_occurrence(span) {
            FlowBindingOccurrence::Resolved(binding) => Ok(Some(binding.clone())),
            FlowBindingOccurrence::Free => Ok(None),
            FlowBindingOccurrence::UnmodeledLocal => Err(FlowBindingMapError::UnmodeledOccurrence),
            FlowBindingOccurrence::Missing => Err(FlowBindingMapError::MissingOccurrence),
        }
    }

    pub(super) fn required_occurrence(
        &self,
        span: FrameSpan,
    ) -> Result<Option<FlowBindingRef>, FlowBindingMapError> {
        match self.occurrence(span) {
            FlowBindingOccurrence::Resolved(binding) => Ok(Some(binding.clone())),
            FlowBindingOccurrence::Free => Ok(None),
            FlowBindingOccurrence::UnmodeledLocal => Err(FlowBindingMapError::UnmodeledOccurrence),
            FlowBindingOccurrence::Missing => Err(FlowBindingMapError::MissingOccurrence),
        }
    }

    pub fn identity(&self, binding: SkeletonBindingId) -> Option<&FlowBindingIdentity> {
        self.identities.get(binding.index())?.as_ref()
    }

    pub fn local(&self, identity: &FlowBindingIdentity) -> Option<SkeletonBindingId> {
        if identity.defining_function != self.function {
            return None;
        }
        let local = self.locals.get(identity.binding_slot as usize).copied()?;
        let stored = self.identity(local)?;
        (stored.name == identity.name && stored.kind == identity.kind).then_some(local)
    }

    pub fn function(&self) -> &FunctionProgramKey {
        &self.function
    }

    pub fn value_count(&self) -> usize {
        self.locals.len()
    }

    /// The runtime variable shared by hoisted redeclarations. Declaration
    /// evidence still uses `identity(binding)`, which remains bijective.
    pub fn canonical_local(&self, binding: SkeletonBindingId) -> SkeletonBindingId {
        self.runtime_locals[binding.index()]
    }

    /// Exact authored declarations of this runtime variable, in source order.
    /// The shared immutable slice is prepared once; lexical shadows never join it.
    pub fn runtime_declarations(&self, binding: SkeletonBindingId) -> &[SkeletonBindingId] {
        let runtime = self.canonical_local(binding).index();
        &self.runtime_declarations[self.declaration_offsets[runtime] as usize
            ..self.declaration_offsets[runtime + 1] as usize]
    }

    /// Precomputed runtime shape; repeated reads never scan alias declarations.
    pub fn runtime_shape(&self, binding: SkeletonBindingId) -> FlowRuntimeBindingShape {
        self.runtime_shapes[self.canonical_local(binding).index()]
    }

    pub fn runtime_identity(&self, binding: SkeletonBindingId) -> Option<&FlowBindingIdentity> {
        self.identity(self.canonical_local(binding))
    }
}

impl SkeletonBindingKind {
    pub const fn function_binding_kind(self) -> Option<FunctionBindingKind> {
        Some(match self {
            Self::Param => FunctionBindingKind::Param,
            Self::Const => FunctionBindingKind::Const,
            Self::Let => FunctionBindingKind::Let,
            Self::Var => FunctionBindingKind::Var,
            Self::NestedFunction => FunctionBindingKind::NestedFunction,
            Self::Class => FunctionBindingKind::Class,
            Self::CatchParam => FunctionBindingKind::CatchParam,
            Self::Enum => FunctionBindingKind::Enum,
            Self::Namespace => FunctionBindingKind::Namespace,
            Self::ImportEquals => FunctionBindingKind::ImportEquals,
            Self::TypeAlias | Self::Interface => return None,
        })
    }
}

#[cfg(test)]
thread_local! {
    static DECLARATION_SPAN_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn record_declaration_span_comparison() {
    DECLARATION_SPAN_COMPARISONS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(super) fn take_declaration_span_comparisons() -> usize {
    DECLARATION_SPAN_COMPARISONS.with(|count| count.replace(0))
}
