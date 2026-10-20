use verter_type_engine::semantic_query::{ClosedObjectProjectionAlternative, ObjectProjectionAlternative};

fn forge_alternative(
    alternative: &ObjectProjectionAlternative,
) -> ClosedObjectProjectionAlternative<'_> {
    ClosedObjectProjectionAlternative { alternative }
}

fn main() {}
