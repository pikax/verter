//! In-gate positive half of the `DisplaySignature` seal (P1-03).
//!
//! The PRIMARY rail for the seal is the ordinary compile of the type itself
//! (private inner field, no `Deserialize`, no `Deref`/`AsRef<str>`/
//! `Into<String>`/`Display`, witness-gated construction) — every gate run
//! exercises it. The trybuild fixtures under `tests/cases/compile-fail/` are
//! the belt-and-braces negative witness in the standalone compile-contract CI lane; this
//! test is their IN-GATE complement, asserting from outside the crate that the
//! deliberately-labelled accessor is the sole route to the display string and
//! that minting flows through a provider-obtained witness.

use verter_type_runtime::protocol::InlayHint;
use verter_type_runtime::protocol::{
    CompletionResult, DisplaySignature, HoverInfo, ProviderDiagnosticContext, RenameLocation,
    SemanticToken, SignatureHelp, TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight,
    TypeLocation,
};
use verter_type_runtime::{ProviderFuture, TypeProvider};

/// A minimal provider impl: possession of a provider impl is the ONLY
/// out-of-crate route to a `DisplaySignatureWireWitness`.
struct WitnessOnlyProvider;

impl TypeProvider for WitnessOnlyProvider {
    fn provider_id(&self) -> &'static str {
        "seal-test"
    }

    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn load_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        _trigger: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        Box::pin(async move {
            Ok(CompletionResult {
                items: Vec::new(),
                is_incomplete: false,
            })
        })
    }

    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async move { Ok(None) })
    }

    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async move { Ok(None) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
        _diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
}

/// The labelled accessor is the sole route to the display string, and the
/// labelled rewrite derivation preserves the brand.
#[test]
fn display_signature_exposes_only_the_labelled_accessor() {
    let signature = DisplaySignature::from_provider_wire(
        WitnessOnlyProvider.provider_wire_witness(),
        "const count: Ref<number>",
    );

    // The ONE read route.
    assert_eq!(signature.as_display_str(), "const count: Ref<number>");

    // The labelled display-domain derivation stays branded and reads back
    // through the same sole accessor.
    let rewritten = signature.with_display_rewrite(|display| display.replace("count", "renamed"));
    assert_eq!(rewritten.as_display_str(), "const renamed: Ref<number>");

    // Serialization is transparent (the wire sees a plain string) — while the
    // REVERSE direction does not exist: `DisplaySignature` implements no
    // `Deserialize` (compile-enforced; witnessed by
    // `display_signature_deserialize.rs`).
    let serialized = serde_json::to_string(&signature).expect("brand serializes transparently");
    assert_eq!(serialized, "\"const count: Ref<number>\"");
}

/// The structured fields ride `HoverInfo` with fail-closed defaults: an
/// engine that supplies nothing yields `None`s, never fabricated values.
#[test]
fn hover_info_structured_fields_default_to_absent() {
    let hover = HoverInfo::default();
    assert!(hover.display_signature.is_none());
    assert!(hover.kind.is_none());
    assert!(hover.documentation.is_none());
    assert!(hover.kind_labeled_signature().is_none());
}
