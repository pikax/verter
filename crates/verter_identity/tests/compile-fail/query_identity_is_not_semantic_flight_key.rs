// `QueryIdentity<Q>` must be distinct from `SemanticFlightKey<Q>`
// (architecture.md §3.1; result-contract-and-flight.md §1:
// `SemanticFlightKey<Q> = (QueryIdentity<Q>, InputBasisId)`). Passing a bare
// `QueryIdentity<Q>` where a `SemanticFlightKey<Q>` is required must fail —
// the basis is a required, not optional, extra part.
use verter_identity::encoding::CanonicalDigest;
use verter_identity::identity::{QueryIdentity, ResultContractId, SemanticFlightKey};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

struct Args(u64);
impl CanonicalEncode for Args {
    const DOMAIN_TAG: &'static str = "compile-fail-test.query-args.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u64(1, self.0);
    }
}

struct FakeQuery;

fn expects_flight_key(_: SemanticFlightKey<FakeQuery>) {}

fn main() {
    let contract = ResultContractId::from_canonical(&Args(1));
    let query_identity = QueryIdentity::<FakeQuery>::compose(
        "compile-fail-test.query.v1",
        CanonicalDigest::of_bytes(b"args"),
        &[],
        &contract,
    );
    expects_flight_key(query_identity);
}
