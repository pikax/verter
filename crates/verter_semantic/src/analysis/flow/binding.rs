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
        let mut identities = Vec::with_capacity(skeleton.bindings.len());
        let mut locals = vec![None; inventory.len()];
        for (ordinal, binding) in skeleton.bindings.iter().enumerate() {
            let Some(kind) = binding.kind.function_binding_kind() else {
                identities.push(None);
                continue;
            };
            let slot = declarations
                .remove(&(binding.span, kind))
                .ok_or(FlowBindingMapError::MissingDeclaration)?;
            let record = &inventory[slot as usize];
            let ordinal =
                u32::try_from(ordinal).map_err(|_| FlowBindingMapError::TooManyBindings)?;
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
        Ok(Self {
            function: function.clone(),
            identities: identities.into(),
            locals: locals.into(),
            runtime_locals: runtime_locals.into(),
        })
    }

    pub fn identity(&self, binding: SkeletonBindingId) -> Option<&FlowBindingIdentity> {
        self.identities.get(binding.index())?.as_ref()
    }

    pub fn local(&self, identity: &FlowBindingIdentity) -> Option<SkeletonBindingId> {
        if identity.defining_function != self.function {
            return None;
        }
        self.locals.get(identity.binding_slot as usize).copied()
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
