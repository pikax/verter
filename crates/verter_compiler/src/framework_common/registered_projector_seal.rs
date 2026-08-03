/// Authority required to run the registered-carrier projector.
///
/// The constructor state is private to this module, so projector code can
/// accept the capability but cannot manufacture one.
pub(super) struct RegisteredProjectorSeal {
    _authorization: (),
}

#[cfg(test)]
pub(super) fn mint_registered_projector_seal_for_tests() -> RegisteredProjectorSeal {
    RegisteredProjectorSeal { _authorization: () }
}

// Reserved ownership boundary for the carrier publication store leader. The
// module is deliberately inaccessible to the projector and has no caller yet;
// the store leader will live here when the first production mint is admitted.
mod carrier_publication_store_leader {
    use super::RegisteredProjectorSeal;

    #[allow(dead_code)]
    fn mint_registered_projector_seal() -> RegisteredProjectorSeal {
        RegisteredProjectorSeal { _authorization: () }
    }
}
