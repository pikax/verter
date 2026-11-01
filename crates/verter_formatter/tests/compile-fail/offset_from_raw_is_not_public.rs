use verter_formatter::coordinates::FormattedByteOffset;

fn main() {
    // Raw integer construction is not part of the public coordinate boundary.
    let _ = FormattedByteOffset::from_raw(0);
}
