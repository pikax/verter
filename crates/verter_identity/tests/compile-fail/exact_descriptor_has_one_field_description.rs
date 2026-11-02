// An exact-size descriptor describes its fields once: the streaming encoding
// is derived from that description, so `from_canonical` and `from_exact` agree
// on the descriptor's bytes. A second, hand-written streaming description
// could drift from it and mint two identities for one descriptor, so it does
// not compile.
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::exact::{CanonicalFieldSink, ExactCanonicalEncode};

struct Drifting;

impl ExactCanonicalEncode for Drifting {
    const DOMAIN_TAG: &'static str = "fixture.drifting.v1";

    fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
        sink.field_u32(1, 1);
    }
}

impl CanonicalEncode for Drifting {
    const DOMAIN_TAG: &'static str = "fixture.drifting.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_u32(1, 2);
    }
}

fn main() {}
