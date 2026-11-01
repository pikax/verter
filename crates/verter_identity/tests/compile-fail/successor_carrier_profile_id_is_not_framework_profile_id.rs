// Nominal distinctness: a `CarrierProfileId` must not type-check where a
// `FrameworkProfileId` is required.
use verter_identity::successor::{CarrierProfileId, FrameworkProfileId, ProfileSchemaEpoch};

fn expects_framework_profile(_: FrameworkProfileId) {}

fn main() {
    let schema = ProfileSchemaEpoch::for_schema_family("successor-compile-fail", 1);
    let carrier = CarrierProfileId::from_carrier("fixture-carrier", &schema);
    expects_framework_profile(carrier);
}
