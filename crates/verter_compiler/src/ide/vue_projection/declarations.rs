//! The Vue public contract filled into the declaration product.
//!
//! The declaration source holds what a consumer of the component can name:
//! the imports and declarations the public types reach (the binder capture's
//! closure), the expose provider over the authored `defineExpose` argument
//! alone, and the public constructor rendered over that provider. The setup
//! statements and the template scope stay in the checking module. A name the
//! public surface reaches that syntax alone cannot publish makes the product
//! unpublishable; the setup body is never pulled in to name it.

use crate::framework_common::virtual_file_naming::published_declaration_identity_for_source;
use crate::ide::projection_declarations::{
    build_declaration_product, DeclarationContractInput, DeclarationDiagnostic,
    DeclarationModuleIdentity, DeclarationProjectionProduct,
};
use crate::ide::vue_projection::binder_capture::BinderCapturePlan;
use crate::ide::vue_projection::public_constructor::{
    PublicDeclarationExtras, VuePublicConstructorContract, EXPOSE_PROVIDER,
};

/// The declaration product of one Vue carrier. `canonical_id` names the
/// carrier; the published file name (`Foo.vue.d.ts`) comes from the
/// carrier's naming policy.
#[must_use]
pub fn project_vue_declarations(
    source: &str,
    canonical_id: &str,
    public: &VuePublicConstructorContract,
    capture: &BinderCapturePlan,
    extras: &PublicDeclarationExtras,
) -> DeclarationProjectionProduct {
    let declaration_file = published_declaration_identity_for_source(canonical_id);
    let constructor = public.declaration_over(EXPOSE_PROVIDER, extras);
    // The provider names only what the capture lifted or retained; the
    // builder refuses an authored name it reaches that the closure lacks.
    let mut public_declaration = public
        .argument_expose_provider(EXPOSE_PROVIDER)
        .unwrap_or_default();
    public_declaration.push_str(constructor.as_deref().unwrap_or_default());
    let module_scope: Vec<(u32, u32)> = capture
        .imports
        .iter()
        .map(|import| (import.span.start, import.span.end))
        .chain(
            capture
                .declarations
                .iter()
                .map(|declaration| (declaration.span.start, declaration.span.end)),
        )
        .collect();
    let mut product = build_declaration_product(
        DeclarationContractInput {
            module: DeclarationModuleIdentity {
                canonical_id: canonical_id.to_owned(),
                declaration_file: declaration_file.clone().unwrap_or_default(),
            },
            public_declaration: &public_declaration,
            module_scope: &module_scope,
            source,
        },
        capture,
    );
    if declaration_file.is_none() {
        product.diagnostics.push(DeclarationDiagnostic {
            start: 0,
            end: 0,
            message: format!("No declaration file is named for {canonical_id}"),
        });
    }
    if constructor.is_none() {
        product.diagnostics.push(DeclarationDiagnostic {
            start: 0,
            end: 0,
            message: "The carrier renders no public constructor".into(),
        });
    }
    product
}
