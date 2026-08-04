use verter_type_expr::facts::{
    ClosedTypeFact, LeafTypeFact, SemanticTypeSource,
};
use verter_type_expr::{
    AuthoredTypeSource, SymbolicEquivalenceKind, SymbolicEquivalenceProof,
};

fn forge(authored: AuthoredTypeSource) {
    let resolved = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
        LeafTypeFact::Primitive(verter_type_expr::PrimitiveName::String),
    ));
    let _forged = SymbolicEquivalenceProof::structural(
        SymbolicEquivalenceKind::ImportedMacroCompound,
        resolved,
        authored,
    );
}

fn main() {}
