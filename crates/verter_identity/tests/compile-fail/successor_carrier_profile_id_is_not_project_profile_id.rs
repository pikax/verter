// Nominal distinctness: a `CarrierProfileId` must not type-check where a
// `ProjectProfileId` is required either.
use verter_identity::successor::{CarrierProfileId, ProfileSchemaEpoch, ProjectProfileId};

fn expects_project_profile(_: ProjectProfileId) {}

fn main() {
    let schema = ProfileSchemaEpoch::for_schema_family("successor-compile-fail", 1);
    let carrier = CarrierProfileId::from_carrier("fixture-carrier", &schema);
    expects_project_profile(carrier);
}
