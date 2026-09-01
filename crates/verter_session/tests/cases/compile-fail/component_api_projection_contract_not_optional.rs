//! A public declaration projection cannot omit its semantic contract.

use verter_session::framework::api_projector::ComponentApiProjection;

fn optional_contract() -> ComponentApiProjection {
    ComponentApiProjection {
        response: loop {},
        contract: None,
        publication_witness: None,
    }
}

fn main() {}
