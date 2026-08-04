use std::sync::Arc;

use verter_session::meta_resolve::TerminalTypeDisplay;
use verter_type_expr::facts::{
    ClosedTypeFact, LeafTypeFact, SemanticTypeSource, SourcePosition,
};
use verter_type_expr::{
    select_type_publication, AuthoredTypeSource, PublicationPolicy, ResolutionExactness,
    ResolutionProvenance, ResolvedTypeAuthority,
};

fn main() {
    let arbitrary = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
        LeafTypeFact::Primitive(verter_type_expr::PrimitiveName::String),
    ));

    // Authored capability construction is locator-only.
    let _forged = AuthoredTypeSource::from_semantic_source(arbitrary.clone());

    let authority = ResolvedTypeAuthority::from_source_position(
        &SourcePosition::Present(arbitrary),
        ResolutionExactness::ExactConcrete,
        ResolutionProvenance::SemanticEvaluator,
        Arc::from([]),
    );

    // Display is not a selector input.
    let _ = select_type_publication(
        &authority,
        None,
        &PublicationPolicy::exact_only(),
        "decoy display",
    );

    let _ = steal_terminal_text;
}

// The raw field needed to forge terminal display is sealed in the sink-owned
// DTO module. Consumers can only use its read-only accessor.
fn steal_terminal_text(display: TerminalTypeDisplay) {
    let _ = display.text;
}
