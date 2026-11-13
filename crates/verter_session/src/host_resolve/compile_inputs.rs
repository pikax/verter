//! The input side of a host compile: one coherent capture of the compiled
//! file, and the framework execution inputs prepared from it.
//!
//! Every compile lane (the caller-supplied request route, the
//! profile-derived virtual-file lane and the runtime render lane) takes its
//! owner/content capture through [`VerterHost::capture_compile_input`]; no
//! lane assembles a [`CompileInput`] of its own.
//!
//! Everything a compile waits on happens here, before the framework
//! backend is called: blocker hydration has ended before the capture
//! ([`VerterHost::capture_compile_input`] takes its outcome by value), and
//! the Vue macro bundle, the dependency axis and the dependency verdict are
//! settled by [`prepare_execution_inputs_pollable`] /
//! [`prepare_execution_inputs`]. The backend call that follows them is a
//! plain synchronous function over the prepared inputs.
//!
//! A prepared result is complete: incomplete or aborted inputs are an
//! error, so the output side never receives a partial macro bundle.

use std::sync::Arc;

use super::compile_blocker_hydration::BlockerHydration;
use super::compile_request_build::svelte_host_execution_inputs;
use crate::block_content::SuppliedBlockScope;
use crate::root_continuation::RootWorkSlot;
use crate::typeinfo::vue_macro_codegen::{VueMacroCodegenDemand, VueMacroCodegenOutput};
use crate::types::*;
use crate::VerterHost;
use verter_compiler::framework_common::{SvelteHostExecutionInputs, VueHostExecutionInputs};
use verter_type_engine::semantic_query::ExecutionAbort;

/// One coherent capture of a compiled file, taken after its blocker
/// hydration ended.
pub(crate) struct CapturedCompileInput {
    /// Every content-determined compiler input, from one source read.
    pub(crate) input: CompileInput,
    /// The source snapshot the capture read, which the attempt binds to.
    pub(crate) source: Arc<verter_scheduler::node::SourceSnapshot>,
    /// The scheduler node incarnation `source` was read from, present when
    /// the capture was asked to mint it ([`SourceWitnessDemand::Mint`]). A
    /// lane that publishes fences its publication on it.
    pub(crate) witness: Option<verter_scheduler::node::SourceWitness>,
    /// The projection of the block content the capture read, taken under
    /// the same admission fence as `input`. A lane that publishes
    /// revalidates it after its compute.
    pub(crate) block_content_stamp: BlockContentHashToken,
    /// How far the blocker hydration before the capture got.
    pub(crate) hydration: BlockerHydration,
}

/// Whether a capture mints the source witness a publishing lane fences on.
/// Minting copies the canonical path, so a lane that publishes nothing
/// declines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceWitnessDemand {
    /// Mint the witness: the lane publishes cache slots and artifacts.
    Mint,
    /// Read the plain snapshot only.
    Skip,
}

/// A compile's prepared framework execution inputs. Always complete.
///
/// Both arms are large and the value is moved once, into the output
/// side, so the arms stay unboxed.
#[allow(clippy::large_enum_variant)]
pub(crate) enum PreparedExecutionInputs {
    /// The Vue carrier's inputs, macro bundle included.
    Vue(VueHostExecutionInputs),
    /// The Svelte carrier's inputs.
    Svelte(SvelteHostExecutionInputs),
}

/// Why a compile's execution inputs could not be prepared.
pub(crate) enum CompileInputsFailure {
    /// A dependency is provably absent: the fatal refusal.
    Missing(DiagnosticsSnapshot),
    /// A dependency is neither proved present nor absent from the inputs
    /// this host holds.
    Incomplete {
        /// Sorted, deduplicated, never empty.
        gaps: Vec<CompileDependencyGap>,
        /// The same diagnostics [`Self::Missing`] would carry.
        diagnostics: DiagnosticsSnapshot,
    },
    /// Cancelled, superseded, shut down or torn: nothing is published.
    Aborted(ExecutionAbort),
}

/// Which execution inputs a compile prepares, chosen by the caller from
/// its bound carrier.
pub(crate) enum ExecutionInputDemand {
    /// The Vue carrier, at the macro bundle depth the compile needs.
    Vue {
        macros: VueMacroCodegenDemand,
        axis: SharedDependencyAxis,
    },
    /// The Svelte carrier.
    Svelte {
        css_hash_override: Option<String>,
        axis: SharedDependencyAxis,
    },
}

/// Whether a compile route restates the compiled file's shared
/// dependency/semantic axis.
///
/// The axis is REPLACED, not merged, and it is the invalidation input for
/// the cache slots the profile-derived lanes publish. A route that
/// publishes a slot therefore MUST restate it. A route that publishes
/// nothing has no slot to invalidate — and restating from its own,
/// possibly narrower, macro demand would overwrite a cached lane's
/// recorded edges with a set that no longer covers them, so a
/// cache-writing lane could then miss an invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SharedDependencyAxis {
    /// Replace the file's recorded transitive dependency/semantic edges
    /// with this compile's own — the profile-derived lanes, which publish
    /// cache slots that invalidate against the axis.
    Restate,
    /// Leave the recorded axis exactly as it stands. The stateless
    /// caller-supplied-request route, which publishes no cache slot: it
    /// owes no invalidation record and must not narrow another lane's.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    LeaveUntouched,
}

impl VerterHost {
    /// Capture `canonical`'s compiler input: take the block-content
    /// admission fence, read ONE source snapshot, build [`CompileInput`]
    /// from it, and drop the fence. Never awaits, and holds no borrow, lock
    /// or fence once it returns.
    ///
    /// `hydration` is the outcome of the blocker hydration that ended
    /// before this capture; the capture carries it so the dependency
    /// verdict can name what hydration could not establish.
    ///
    /// `witness_demand` says whether the capture mints the source witness.
    ///
    /// The admission fence is re-entrant, so a lane that already holds it
    /// across its own cache consult may capture under it.
    ///
    /// # Errors
    ///
    /// [`HostError::MissingSource`] when no source is registered under
    /// `canonical`, or the block-content capture's own error.
    pub(crate) fn capture_compile_input(
        &self,
        canonical: &str,
        scope: SuppliedBlockScope<'_>,
        hydration: BlockerHydration,
        witness_demand: SourceWitnessDemand,
    ) -> Result<CapturedCompileInput, HostError> {
        use crate::host_executor::HostSourceData;

        #[cfg(test)]
        self.test_force
            .compile_input_capture_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let missing = || HostError::MissingSource {
            canonical_id: canonical.to_owned(),
        };
        let _block_content_capture_fence = self.block_content.admission_fence.lock();

        // ── ONE coherent source snapshot ──
        // Every content-determined input derives from this single read, so
        // the bytes and the analysis beside them cohere. A minted witness
        // names the node incarnation the read came from.
        let (source, witness) = match witness_demand {
            SourceWitnessDemand::Mint => {
                let verter_scheduler::node::WitnessedSource { snapshot, witness } = self
                    .scheduler
                    .try_get_witnessed_source(canonical)
                    .ok_or_else(missing)?;
                (snapshot, Some(witness))
            }
            SourceWitnessDemand::Skip => (
                self.scheduler
                    .try_get_source(canonical)
                    .ok_or_else(missing)?,
                None,
            ),
        };
        let efs = self
            .effective_file_state_from_snapshot(&source, canonical, None)
            .ok_or_else(missing)?;
        let parse = &source
            .downcast_data::<HostSourceData>()
            .ok_or_else(missing)?
            .parse;
        let style_content =
            self.capture_compiler_style_content(canonical, &parse.style_analyses, scope);
        let block_content = self.capture_compiler_block_content(canonical, scope)?;
        let input = CompileInput {
            canonical_id: canonical.to_owned(),
            source: efs.source,
            whole_hash: efs.whole_hash,
            meta: parse.meta.clone(),
            parse_diagnostics: parse.parse_diagnostics.clone(),
            src_blocks: parse.src_blocks.clone(),
            external_requests: parse.external_requests.clone(),
            has_supplied_block_content: block_content.has_supplied,
            block_content_inputs: block_content.inputs,
            macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
            script_imports: efs.script_analysis.imports.clone(),
            script_macros: efs.script_analysis.macros.clone(),
            script_bindings: efs.script_analysis.bindings.clone(),
            script_macro_usage: efs.script_analysis.macro_usage.clone(),
            script_vue_api_calls: efs.script_analysis.vue_api_calls.clone(),
            framework_parse: efs.framework_parse,
            style_v_bind_vars: style_content.v_bind_vars,
            style_v_bind_usage_complete: style_content.usage_complete,
            prepared_styles: parse.prepared_styles.clone(),
        };
        Ok(CapturedCompileInput {
            input,
            source,
            witness,
            block_content_stamp: block_content.stamp,
            hydration,
        })
    }
}

/// Prepare `captured`'s execution inputs for a host root: the Vue macro
/// bundle is produced by the pollable path, which hands the root's driver
/// its producer and dispatch steps through `work`.
pub(crate) async fn prepare_execution_inputs_pollable<'h>(
    host: &'h VerterHost,
    captured: &CapturedCompileInput,
    demand: ExecutionInputDemand,
    work: &RootWorkSlot<'h>,
) -> Result<PreparedExecutionInputs, CompileInputsFailure> {
    let snapshot = &captured.input;
    match demand {
        ExecutionInputDemand::Vue { macros, axis } => {
            let macro_output = host
                .produce_vue_macro_codegen_pollable(&snapshot.canonical_id, macros, work)
                .await
                .map_err(CompileInputsFailure::Aborted)?;
            vue_execution_inputs_from_macro_output(
                host,
                snapshot,
                macro_output,
                axis,
                captured.hydration.gaps(),
            )
            .map(PreparedExecutionInputs::Vue)
        }
        ExecutionInputDemand::Svelte {
            css_hash_override,
            axis,
        } => Ok(PreparedExecutionInputs::Svelte(
            prepare_svelte_execution_inputs(host, snapshot, css_hash_override, axis),
        )),
    }
}

/// Prepare `captured_input`'s execution inputs on this thread: the profile
/// lanes' synchronous entry. These lanes hydrate before their capture, but
/// their compile entry takes the captured input alone, so the verdict names
/// only the gaps its own walk finds.
pub(crate) fn prepare_execution_inputs(
    host: &VerterHost,
    captured_input: &CompileInput,
    demand: ExecutionInputDemand,
) -> Result<PreparedExecutionInputs, CompileInputsFailure> {
    match demand {
        ExecutionInputDemand::Vue { macros, axis } => {
            let macro_output = host
                .produce_vue_macro_codegen(&captured_input.canonical_id, macros)
                .map_err(CompileInputsFailure::Aborted)?;
            vue_execution_inputs_from_macro_output(host, captured_input, macro_output, axis, &[])
                .map(PreparedExecutionInputs::Vue)
        }
        ExecutionInputDemand::Svelte {
            css_hash_override,
            axis,
        } => Ok(PreparedExecutionInputs::Svelte(
            prepare_svelte_execution_inputs(host, captured_input, css_hash_override, axis),
        )),
    }
}

/// The Vue execution inputs over one produced macro bundle: decide the
/// dependency verdict, restate the dependency axis (when the route owns
/// one), refuse on this file's macro dependency diagnostics, and build the
/// ephemeral execution-input carrier.
///
/// The axis restatement runs BEFORE the dependency refusal so a compile
/// refused for an unresolvable macro type still records the dependencies
/// whose repair must invalidate it — otherwise fixing the missing type
/// would never re-run this file.
//
// `VueHostExecutionInputs` carries one `#[cfg(any(test, feature =
// "test-support"))]` field, and that cfg is evaluated in `verter_compiler`,
// not here: `verter_session/test-support` forwards to `verter_scheduler`, so
// nothing this crate can spell predicts whether the field exists. The struct
// update tail is what makes the literal correct in every combination --
// `Default` supplies exactly the fields that were compiled. Naming the field
// under a local cfg instead fails `cargo check --workspace` with E0560.
#[allow(clippy::needless_update)]
fn vue_execution_inputs_from_macro_output(
    host: &VerterHost,
    snapshot: &CompileInput,
    macro_output: VueMacroCodegenOutput,
    axis: SharedDependencyAxis,
    hydration_gaps: &[CompileDependencyGap],
) -> Result<VueHostExecutionInputs, CompileInputsFailure> {
    let verdict = super::vue_macro_dependency_diagnostics::collect(
        host,
        snapshot,
        &macro_output,
        hydration_gaps,
    );
    if axis == SharedDependencyAxis::Restate {
        let transitive_macro_type_deps: std::collections::BTreeSet<String> =
            macro_output.transitive_canonicals.iter().cloned().collect();
        host.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &transitive_macro_type_deps,
        );
    }
    if !verdict.diagnostics.is_empty() {
        let diagnostics = DiagnosticsSnapshot::from_vec(verdict.diagnostics);
        return Err(match verdict.incomplete {
            Some(gaps) => CompileInputsFailure::Incomplete { gaps, diagnostics },
            None => CompileInputsFailure::Missing(diagnostics),
        });
    }
    // The host-resolved Vue cross-file inputs ride on the typed,
    // ephemeral `VueExecutionInputs` carrier — excluded from
    // `CompileRequest` identity, and reachable only from here.
    let vue_facts = verter_compiler::compile::types::VueExecutionInputs {
        prop_constness_overrides: None, // populated by the cross-file optimizer
        style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
        style_v_bind_usage_complete: Some(snapshot.style_v_bind_usage_complete),
        template_binding_metadata: None,
        template_used_vars: None,
        runtime_template_hole: false,
        runtime_inline_template_chunk: false,
        prepared_styles: snapshot.prepared_styles.clone(),
    };
    let (style_specifiers, custom_specifiers) = crate::compile::vue_main_host_identifiers(
        &snapshot.canonical_id,
        snapshot.meta.style_langs.len(),
        snapshot.meta.custom_types.len(),
        &snapshot.meta,
    );
    Ok(VueHostExecutionInputs {
        block_content: snapshot.block_content_inputs.clone(),
        vue_facts: Some(vue_facts),
        // The one macro-semantic channel: exactly what the TypeInfo
        // producer built for this compile's target demand, staged into
        // the compile transaction by the compiler-side entry.
        vue_macros: macro_output.compiler_input(),
        prepared_styles: snapshot.prepared_styles.clone(),
        canonical_id: snapshot.canonical_id.clone(),
        style_specifiers,
        custom_specifiers,
        project_identity: host.host_view_project_identity_for(&snapshot.canonical_id),
        want_main: true,
        has_script: snapshot.meta.has_script,
        has_template: snapshot.meta.has_template,
        script_lang: snapshot.meta.script_lang.clone(),
        ..Default::default()
    })
}

/// The Svelte arm's own execution-input preparation.
///
/// This arm's execution inputs come entirely from the presented artifact
/// and the caller's inputs, so it contributes no cross-file semantic type
/// dependencies. A route that OWNS the axis still restates it with that
/// empty contribution: the axis is REPLACED, not merged, so skipping the
/// restatement would leave a previous compute's edges standing for this
/// file. Callers prepare before decoding their demand for the same reason
/// the Vue arm restates before its dependency refusal — a refused compile
/// still restates the axis.
fn prepare_svelte_execution_inputs(
    host: &VerterHost,
    snapshot: &CompileInput,
    css_hash_override: Option<String>,
    axis: SharedDependencyAxis,
) -> SvelteHostExecutionInputs {
    if axis == SharedDependencyAxis::Restate {
        host.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &std::collections::BTreeSet::new(),
        );
    }
    svelte_host_execution_inputs(snapshot, css_hash_override)
}
