//! Compile-fail fixture: the certified engine binding's fields are PRIVATE,
//! so a caller cannot assemble a `CertifiedTypeEngineBinding` by struct
//! literal — the zero-mint forge that would bypass the witness chain
//! (resolver → `ProjectBinding` → `EnsureProject` → `BoundProject` →
//! `certify`). If the fields were widened to `pub`, this fixture would
//! COMPILE and trybuild would turn red.

use std::sync::Arc;

use verter_identity::identity::{InputBasisId, ProviderContractId};
use verter_session::semantic_capability::CertifiedTypeEngineBinding;

struct Nothing;

impl verter_identity::encoding::CanonicalEncode for Nothing {
    const DOMAIN_TAG: &'static str = "verter.session.tests.certified_binding.forge.v1";

    fn encode_fields(&self, encoder: &mut verter_identity::encoding::CanonicalEncoder) {
        encoder.field_bytes(1, b"nothing");
    }
}

fn forge_binding() -> CertifiedTypeEngineBinding {
    CertifiedTypeEngineBinding {
        project: Arc::from("file:///forged/tsconfig.json"),
        observed_profile: ProviderContractId::from_canonical(&Nothing),
        input_basis: InputBasisId::from_canonical(&Nothing),
    }
}

fn main() {
    let _ = forge_binding();
}
