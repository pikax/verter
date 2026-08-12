// `InputBasisId` must be distinct from `QueryIdentity<Q>`
// (architecture.md §3.1: "`InputBasisId` scopes in-flight semantic
// production but is not part of cross-snapshot candidate lookup"). Passing
// an `InputBasisId` where a `QueryIdentity<Q>` is required must fail.
use verter_identity::identity::{InputBasisId, QueryIdentity};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

struct Args(u64);
impl CanonicalEncode for Args {
    const DOMAIN_TAG: &'static str = "compile-fail-test.basis-args.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u64(1, self.0);
    }
}

struct FakeQuery;

fn expects_query_identity(_: QueryIdentity<FakeQuery>) {}

fn main() {
    let basis = InputBasisId::from_canonical(&Args(1));
    expects_query_identity(basis);
}
