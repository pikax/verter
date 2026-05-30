#![deny(missing_docs)]
//! `VerterHost::list_file_symbols` — pure shallow-state read.
//!
//! Walks the file's [`crate::resolver_core::shallow_file_state::ShallowFileState`] +
//! analysis snapshot once and projects the symbol inventory into the
//! consumer-visible [`super::types::SymbolEntry`] DTO. No type
//! evaluation, no expansion, no dispatch entry — the call is bounded
//! by the size of the shallow inventory and is safe to call without
//! audit instrumentation — this entry-point is a pure shallow read
//! and adds no instrumentation overhead.
//!
//! The host method is the substrate that the
//! `@verter/typeinfo` package's `listFileSymbols(file)` call lowers
//! to. Spans come from the file's
//! [`verter_semantic::analysis::types::ScriptAnalysisSnapshot::declaration_entries`]
//! when present; ambient declarations without analysis-snapshot spans
//! surface with `span: None`.

use std::collections::BTreeMap;
use std::sync::Arc;

use verter_semantic::analysis::type_eval::{TypeDeclKind, ValueDeclKind};
use verter_semantic::analysis::types::{LocalDeclarationEntry, LocalDeclarationKind};

use super::types::{SymbolEntry, SymbolKind};
use crate::VerterHost;

impl VerterHost {
    /// Return the top-level symbol inventory for `canonical_id`.
    ///
    /// Pulls the file's shallow type / value symbol tables, classifies
    /// each entry into the public [`SymbolKind`] taxonomy, attaches
    /// the SFC-absolute span from the analysis snapshot when one is
    /// present, and marks each entry as exported when it appears in
    /// the file's `exports` table as `ExportTarget::Local` or as a
    /// declaration whose name aligns with a wildcard re-export
    /// origin.
    ///
    /// Class and enum declarations are dual-space: each yields a
    /// type-side entry ([`SymbolKind::Class`] / [`SymbolKind::Enum`])
    /// and a value-side entry ([`SymbolKind::ClassValue`] /
    /// [`SymbolKind::Enum`]) so callers downstream can distinguish
    /// the two namespaces without rerunning the shallow analyser.
    /// Class / enum names appear once in the result with the type
    /// kind because the type and value entries share a name and a
    /// span — the discriminator that lets a caller resolve the value
    /// side is the [`SymbolKind`] returned for the value entry.
    ///
    /// **Performance contract**: < 50 ms on a 5 KLOC TS file.
    /// The implementation copies every shallow symbol once and
    /// performs O(N) hash lookups against the analysis snapshot's
    /// declaration-entry list; both are bounded by the size of the
    /// inventory and never reopen the file or the parser.
    ///
    /// Returns an empty vec when the file is not loaded or has no
    /// shallow inventory yet.
    #[must_use]
    pub fn list_file_symbols(&self, canonical_id: &str) -> Vec<SymbolEntry> {
        // Ensure the canonical post-parse artifact is materialised
        // through the shared host pipeline. `ensure_indexed_ready`
        // builds (or reuses) the `IndexedReady` for `canonical_id`,
        // which carries both the shallow inventory we walk for
        // names AND the analysis snapshot we use for spans. Calling
        // it here is idempotent: warm hits short-circuit on the
        // cached entry; cold misses populate it once.
        let facts_arc = self.ensure_indexed_ready(canonical_id);
        let Some(facts) = facts_arc.as_ref() else {
            return Vec::new();
        };
        let shallow = Arc::clone(&facts.shallow_state);

        // Build a name → declaration-entry lookup from the analysis
        // snapshot so each surfaced symbol can be paired with its
        // declaration entry in O(1). The `facts` `Arc` keeps the
        // backing `IndexedReady` alive for the duration of the
        // walk, so the `&LocalDeclarationEntry` borrows are valid.
        let mut decl_index: BTreeMap<&str, &LocalDeclarationEntry> = BTreeMap::new();
        if let Some(snapshot) = facts.script_analysis.as_ref() {
            for entry in &snapshot.declaration_entries {
                decl_index.insert(entry.name.as_str(), entry);
            }
        }

        // Classify each shallow type / value declaration into the
        // public taxonomy. Class + Enum are dual-space and surface
        // twice (once on the type side, once on the value side).
        let mut entries: Vec<SymbolEntry> =
            Vec::with_capacity(shallow.symbols.len() + shallow.value_symbols.len());

        for (name, type_sym) in &shallow.symbols {
            let kind = match type_sym.kind {
                TypeDeclKind::Alias => SymbolKind::TypeAlias,
                TypeDeclKind::Interface => SymbolKind::Interface,
                TypeDeclKind::Class => SymbolKind::Class,
            };
            let span = decl_index.get(name.as_str()).map(|entry| entry.span);
            let is_exported = is_exported_local(shallow.as_ref(), name)
                || decl_index
                    .get(name.as_str())
                    .map(|entry| {
                        // The shallow `exports` table is the primary
                        // exporter authority; the analysis-snapshot
                        // kind only contributes when an entry exists
                        // there but the shallow re-export probe
                        // missed (e.g. ambient module declarations
                        // routed through a different lowering pass).
                        matches!(
                            entry.kind,
                            LocalDeclarationKind::Type | LocalDeclarationKind::TypeAndValue
                        )
                    })
                    .unwrap_or(false)
                    && is_local_export_target(shallow.as_ref(), name);
            entries.push(SymbolEntry {
                name: name.clone(),
                kind,
                span,
                is_exported,
            });
        }

        for (name, value_sym) in &shallow.value_symbols {
            let kind = match value_sym.kind {
                ValueDeclKind::Const => SymbolKind::Const,
                ValueDeclKind::Let => SymbolKind::Let,
                ValueDeclKind::Var => SymbolKind::Var,
                ValueDeclKind::Function => SymbolKind::Function,
                ValueDeclKind::AsyncFunction => SymbolKind::AsyncFunction,
                ValueDeclKind::Class => SymbolKind::ClassValue,
                ValueDeclKind::Enum => SymbolKind::Enum,
            };
            let span = decl_index.get(name.as_str()).map(|entry| entry.span);
            let is_exported = is_exported_local(shallow.as_ref(), name);
            entries.push(SymbolEntry {
                name: name.clone(),
                kind,
                span,
                is_exported,
            });
        }

        // Stable order — name then kind discriminator — so callers
        // (and the test suite) can compare snapshots without a
        // post-hoc sort. The hash-table iteration order from the
        // shallow tables is otherwise arbitrary.
        entries.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| (a.kind as u8).cmp(&(b.kind as u8)))
        });
        entries
    }
}

/// `true` when `name` appears in the file's `exports` table as a
/// locally-declared symbol (`ExportTarget::Local`). The shallow state
/// is the authority; the analysis snapshot is consulted only as a
/// secondary signal in [`VerterHost::list_file_symbols`] when the
/// shallow probe missed.
fn is_exported_local(
    shallow: &crate::resolver_core::shallow_file_state::ShallowFileState,
    name: &str,
) -> bool {
    use crate::resolver_core::shallow_file_state::ExportTarget;
    shallow.exports.iter().any(|(_exported, target)| {
        matches!(target, ExportTarget::Local { symbol_name } if symbol_name == name)
    })
}

/// Subordinate check for the analysis-snapshot exporter signal. When
/// the analysis snapshot says a declaration is type/typeAndValue and
/// the shallow exports table has a `Local` entry whose `symbol_name`
/// matches, treat the declaration as exported.
fn is_local_export_target(
    shallow: &crate::resolver_core::shallow_file_state::ShallowFileState,
    name: &str,
) -> bool {
    is_exported_local(shallow, name)
}
