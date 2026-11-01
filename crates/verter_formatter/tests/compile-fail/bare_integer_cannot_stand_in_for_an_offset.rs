use verter_formatter::coordinates::AuthoredFormatByteOffset;

fn authored_only(_offset: AuthoredFormatByteOffset) {}

fn main() {
    // A bare integer cannot stand in for a domain offset.
    authored_only(0);
}
