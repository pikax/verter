use verter_formatter::coordinates::{AuthoredFormatByteRange, FormattedByteOffset};

fn formatted_offset() -> FormattedByteOffset {
    unreachable!()
}

fn main() {
    // An authored range cannot be built from formatted offsets.
    let _ = AuthoredFormatByteRange::new(formatted_offset(), formatted_offset());
}
