use verter_formatter::provenance::FormatProvenanceId;

fn consumer(_id: FormatProvenanceId) {}

fn main() {
    // A bare integer cannot stand in for a provenance identity.
    consumer(0);
}
