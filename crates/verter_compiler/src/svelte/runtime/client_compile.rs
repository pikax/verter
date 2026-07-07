//! The carrier-facing client compile ENTRY of the Svelte runtime backend.
//!
//! Extracted from the runtime module root (the file-size guard boundary):
//! [`compile_client`] — the REFUSE-BY-DEFAULT pipeline driver (official-reject
//! gate → parse-domain gate → runtime lowering → default-deny classification →
//! narrow-plan projection → static-template + topology planning → emission) —
//! and its typed [`ClientCompileError`] outcome.

use oxc_allocator::Allocator;

use crate::svelte::parser::ParsedSvelte;
use verter_span::Span;

use super::client;
use super::client_plan;
use super::client_surface;
use super::official_reject;
use super::official_rule::OfficialRejection;
use super::parse_refusal::parse_domain_gate;
use super::{
    lower_parsed_svelte_to_ir, plan_client_topology, plan_static_templates, RuntimeLoweringErrors,
    SvelteRuntimeOptions, UnsupportedSvelteRuntimeSurface,
};

/// The outcome of [`compile_client`] when the client module cannot be emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCompileError {
    /// The runtime lowering itself failed (a malformed construct) — carries the
    /// collected lowering diagnostics.
    Lowering(RuntimeLoweringErrors),
    /// The component uses a runtime surface this backend does not yet emit — fails
    /// closed with the typed reason (never a silent empty module).
    Unsupported(UnsupportedSvelteRuntimeSurface),
    /// The component is MALFORMED Svelte the official `svelte@5.56.3` compiler also
    /// COMPILE-ERRORS (a duplicate declaration, a `$`-prefixed binding, a duplicate /
    /// mis-`context`-ed `<script>`, an invalid HTML placement, a global `$foo`
    /// reference). Accepting it would change the observable contract from "compile
    /// error, no module" to "module exists", so it fails closed with the typed
    /// official-reject rejection (the rule class + its exact official code) — never a
    /// `Main`. The official-reject parity quadrant.
    OfficialReject(OfficialRejection),
}

impl From<UnsupportedSvelteRuntimeSurface> for ClientCompileError {
    /// Convert a REWRITER-CHANNEL refusal to the client compile error. The fallible
    /// expression rewriter reports through `UnsupportedSvelteRuntimeSurface`, but a
    /// [`UnsupportedSvelteRuntimeSurface::OfficialReject`] carrier is a mid-rewrite
    /// OFFICIAL compile-error (a `$$`-member of a `$props()` rest / whole-object
    /// binding → `props_illegal_name`), so it becomes a real
    /// [`ClientCompileError::OfficialReject`] carrying the exact official code — NOT
    /// the generic unsupported quadrant. Every other surface is a genuine
    /// unsupported feature.
    fn from(surface: UnsupportedSvelteRuntimeSurface) -> Self {
        match surface {
            UnsupportedSvelteRuntimeSurface::OfficialReject { rejection, .. } => {
                ClientCompileError::OfficialReject(rejection)
            }
            other => ClientCompileError::Unsupported(other),
        }
    }
}

/// Compile a parsed Svelte component into the `svelte/internal/client` JS module
/// (the carrier-facing entry).
///
/// Runs the full pipeline — runtime lowering → static-template planning →
/// client-topology planning → [`emit_client_module`] — and returns the emitted
/// module, or a typed [`ClientCompileError`] (a lowering failure, or an
/// unsupported surface that fails closed). `ssr` requests the server backend
/// (fails closed until the server backend lands).
pub fn compile_client<'a>(
    source: &'a str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
    alloc: &'a Allocator,
    ssr: bool,
) -> Result<client::ClientModule, ClientCompileError> {
    // The REFUSE-BY-DEFAULT pipeline. Each stage is a choke point: an unsupported
    // surface fails closed BEFORE the next stage, so the narrow plan the emitter
    // consumes can ONLY describe a fully-supported component — emit-by-default is
    // structurally impossible.
    //
    // (0) SSR requests the server backend (fails closed until it lands).
    if ssr {
        return Err(ClientCompileError::Unsupported(
            UnsupportedSvelteRuntimeSurface::ServerGenerate {
                span: Span::new(0, 0),
            },
        ));
    }
    // (1) `official_reject_gate` — the OFFICIAL-REJECT parity gate. Refuse the
    // MALFORMED-input classes official ALSO compile-errors (a duplicate / mis-context
    // `<script>`, a `$`-prefixed binding, a duplicate accepted declaration, a global
    // `$foo` / `$$foo` reference, an invalid HTML placement) FIRST, so a genuinely
    // malformed component is rejected for being malformed — not later mis-attributed
    // to an unsupported feature, and never accepted as a divergent `Main`.
    if let Some(rejection) =
        official_reject::official_reject_gate_with_runes(source, parsed, opts.runes)
    {
        return Err(ClientCompileError::OfficialReject(rejection));
    }
    // (2) `parse_domain_gate` — refuse the PARSE-DOMAIN surfaces the runtime IR
    // does not carry (a top-level `<style>` (5l), a `<svelte:options>` axis beyond
    // `runes` / `customElement` (5m), a dev-mode codegen request (5k)) BEFORE
    // lowering, so a lossy lowering cannot hide them. A VALID `customElement`
    // value passes this gate and is LOWERED by the native client path (the
    // parser-retained descriptor); only its invalid forms remain rejected — as
    // exact-code official rejects by the gate above, never here.
    if let Some(surface) = parse_domain_gate(source, parsed, opts) {
        return Err(ClientCompileError::Unsupported(surface));
    }
    // (2) Lower to the BROAD runtime IR (the shared substrate). The broad IR may
    // exist; it just never reaches emission.
    let ir = lower_parsed_svelte_to_ir(source, parsed, opts, alloc)
        .map_err(ClientCompileError::Lowering)?;
    // (3) `ClientSyntaxSurface::classify` — the DEFAULT-DENY classifier. It accepts
    // ONLY when every node / attr / script-item is in the supported allowlist; the
    // first unsupported surface fails closed (no wildcard accept arm). The
    // discriminating `From` conversion routes a classifier-detected OFFICIAL
    // reject (the `OfficialReject` carrier — e.g. a runes-mode `export let` /
    // `$:` detected against the FINAL inferred mode) to
    // `ClientCompileError::OfficialReject`, everything else to `Unsupported`.
    let classified =
        client_surface::ClientSyntaxSurface::classify(&ir).map_err(ClientCompileError::from)?;
    // (4) `SupportedClientIr::build` — the semantic projection. It decides which
    // interpolations are ACTUALLY reactive (a non-reactive one fails closed),
    // validates lvalues, and rewrites every script item + op through the FALLIBLE
    // rewriter into the NARROW `ClientModulePlan`.
    // The rewriter reports through `UnsupportedSvelteRuntimeSurface`; the
    // discriminating `From` conversion routes a mid-rewrite official-reject carrier
    // (a `$$`-member of a rest / whole-object binding) to
    // `ClientCompileError::OfficialReject`, everything else to `Unsupported`.
    let plan = client_plan::SupportedClientIr::build(&classified, &ir)
        .map_err(ClientCompileError::from)?;
    // (5) Plan the static templates + topology, then emit from the NARROW plan only.
    let html_plan = plan_static_templates(&ir);
    let topology = plan_client_topology(&ir, &html_plan);
    Ok(client::emit_client_module(&plan, &html_plan, &topology))
}
