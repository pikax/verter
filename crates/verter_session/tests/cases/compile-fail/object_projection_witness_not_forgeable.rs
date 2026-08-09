use verter_session::semantic_query::{
    ClosedObjectProjectionAlternative, ClosedObjectProjectionFormula,
    ObjectProjectionAlternative, ObjectProjectionFormula,
};

fn forge_alternative(
    alternative: &ObjectProjectionAlternative,
) -> ClosedObjectProjectionAlternative<'_> {
    ClosedObjectProjectionAlternative { alternative }
}

fn forge_formula(formula: &ObjectProjectionFormula) -> ClosedObjectProjectionFormula<'_> {
    ClosedObjectProjectionFormula { formula }
}

fn main() {}
