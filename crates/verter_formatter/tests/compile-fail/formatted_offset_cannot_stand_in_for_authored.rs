use verter_formatter::coordinates::{AuthoredFormatByteOffset, FormattedByteOffset};

fn authored_only(_offset: AuthoredFormatByteOffset) {}

fn formatted_offset() -> FormattedByteOffset {
    // However a formatted offset is obtained inside the formatter, it is
    // a formatted offset.
    unreachable!()
}

fn main() {
    // A formatted offset cannot stand in for an authored offset.
    authored_only(formatted_offset());
}
