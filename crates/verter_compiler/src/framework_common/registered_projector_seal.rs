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

pub(super) fn mint_registered_projector_seal_for_store_leader() -> RegisteredProjectorSeal {
    RegisteredProjectorSeal { _authorization: () }
}
