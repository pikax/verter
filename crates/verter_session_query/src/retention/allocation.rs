//! Fact-aware allocation footprints. The declared conventions and the
//! generic helpers live in [`verter_retention::allocation`], re-exported here.

pub use verter_retention::allocation::*;

use std::sync::Arc;

use crate::facts::fact_cache::ReadSetSignature;
use crate::facts::registry::FactKey;
use crate::facts::resolution::ResolutionFactKey;
use crate::facts::version::{FactVersionRef, ProgramAnalysisFactRef, ResolveImportsFactRef};

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
            function.canonical_id.offer(aliases)
                + function.merged_symbol_name.offer(aliases)
                + match &function.function_part {
                    verter_type_expr::facts::FunctionPartIdentity::Member { member_path } => {
                        member_path.offer(aliases)
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
                // An owned name: charged by its own cell, collected here.
                bytes += text.offer(aliases);
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
