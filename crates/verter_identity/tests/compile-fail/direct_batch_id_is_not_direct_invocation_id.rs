// Each `ParseOwnerDomainId` variant must carry ITS OWN owner identity
// (`contracts/parse-ownership.md` §2), because the variant plus its payload
// is what decides sharing. Constructing the variant is what makes this
// fixture sensitive to the declaration rather than to two standalone
// newtypes: it also fails if `DirectInvocation` loses its payload, or is
// wired to the wrong identity type.
use verter_identity::identity::{DirectBatchId, ParseOwnerDomainId};

fn main() {
    let _ = ParseOwnerDomainId::DirectInvocation(DirectBatchId(1));
}
