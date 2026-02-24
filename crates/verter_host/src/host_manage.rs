//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use crate::hash::compile_profile_hash;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

impl VerterHost {
    /// Returns a serializable snapshot of the file's static analysis data.
    /// Returns `None` if the file doesn't exist.
    /// When `eager_analysis` is false, computes analysis on demand from stored source.
    ///
    /// Template analysis is included when it has been computed during a prior
    /// compilation (requires template scope flags and a `get_virtual_file()` call).
    pub fn get_analysis(&self, canonical_or_alias: &str) -> Option<FileAnalysisSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let entry = files.get(&canonical)?;

        // If analysis wasn't fully computed during upsert, compute missing parts on demand
        let scope = self.config.effective_scope();
        if entry.file_kind == FileKind::VueSfc
            && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
        {
            let source = entry.source.clone();
            let stored_script = entry.script_analysis.clone();
            let template = entry.template_analysis.clone();
            drop(files);

            let script_analysis = if !scope.needs_script_analysis() {
                crate::parse::build_script_analysis_from_source(&source)
            } else {
                // Script analysis was already computed during upsert
                stored_script
            };
            let style_analyses = crate::parse::build_style_analyses_from_source(&source);
            return Some(FileAnalysisSnapshot {
                imports: script_analysis.imports,
                bindings: script_analysis.bindings,
                macros: script_analysis.macros,
                macro_type_deps: script_analysis.macro_type_deps,
                script_flags: script_analysis.flags.bits(),
                styles: style_analyses,
                template,
            });
        }

        Some(FileAnalysisSnapshot {
            imports: entry.script_analysis.imports.clone(),
            bindings: entry.script_analysis.bindings.clone(),
            macros: entry.script_analysis.macros.clone(),
            macro_type_deps: entry.script_analysis.macro_type_deps.clone(),
            script_flags: entry.script_analysis.flags.bits(),
            styles: entry.style_analyses.clone(),
            template: entry.template_analysis.clone(),
        })
    }

    /// Returns stored diagnostics for a file+profile without triggering compilation.
    /// Returns `None` if the file doesn't exist or has no diagnostics for this profile.
    pub fn get_diagnostics(
        &self,
        canonical_or_alias: &str,
        profile: &CompileProfile,
    ) -> Option<DiagnosticsSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let profile_hash = compile_profile_hash(profile);
        let files = read_lock(&self.files);
        let entry = files.get(&canonical)?;
        entry.latest_diagnostics.get(&profile_hash).cloned()
    }

    /// Remove a file from the host, cleaning up aliases, dependencies,
    /// and invalidating compile slots of any dependents.
    pub fn remove(&self, canonical_or_alias: &str) -> Option<HostRemoveResult> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        let removed = {
            let mut files = write_lock(&self.files);
            files.remove(&canonical)
        }?;

        {
            let mut alias_map = write_lock(&self.alias_to_canonical);
            for alias in &removed.aliases {
                alias_map.remove(alias);
            }
        }

        // Collect dependents before modifying the reverse_dependencies map.
        let dependents = {
            let rev = read_lock(&self.reverse_dependencies);
            rev.get(&canonical).cloned().unwrap_or_default()
        };

        {
            let mut rev = write_lock(&self.reverse_dependencies);
            for dep in &removed.dependencies {
                if let Some(owners) = rev.get_mut(dep) {
                    owners.remove(&canonical);
                    if owners.is_empty() {
                        rev.remove(dep);
                    }
                }
            }
            rev.remove(&canonical);
        }

        // Invalidate compile slots of files that depended on the removed file.
        if !dependents.is_empty() {
            let mut files = write_lock(&self.files);
            for owner in dependents {
                if let Some(file) = files.get_mut(&owner) {
                    file.compile_slots.clear();
                }
            }
        }

        Some(HostRemoveResult {
            canonical_id: canonical,
        })
    }

    /// Provide caller-resolved import dependency canonical IDs.
    /// Called after upsert() when the caller resolves non-relative import paths
    /// (tsconfig paths, vite aliases, etc.) using bundler/LSP resolution.
    pub fn set_import_dependencies(&self, canonical_or_alias: &str, resolved_deps: Vec<String>) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Single write lock to avoid TOCTOU race between read-read-write.
        let (old_deps, new_deps) = {
            let mut files = write_lock(&self.files);
            let Some(entry) = files.get_mut(&canonical) else {
                return;
            };
            let old_deps = entry.dependencies.clone();
            for dep in resolved_deps {
                entry.dependencies.insert(dep);
            }
            let new_deps = entry.dependencies.clone();
            (old_deps, new_deps)
        };

        self.update_reverse_deps(&canonical, &old_deps, &new_deps);
    }
}
