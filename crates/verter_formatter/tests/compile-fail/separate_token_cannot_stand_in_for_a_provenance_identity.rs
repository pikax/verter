use verter_formatter::provenance::FormatProvenanceId;

struct OtherToken(u32);

fn consumer(_id: FormatProvenanceId) {}

fn main() {
    // A separately declared token type cannot stand in either.
    consumer(OtherToken(0));
}
