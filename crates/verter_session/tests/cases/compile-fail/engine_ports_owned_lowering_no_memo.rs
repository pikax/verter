use verter_session::for_tests::OwnedLowering;

fn cannot_escape<P: OwnedLowering + ?Sized>(port: &P) {
    if let Some((_record, source)) = port.indexed_expression_source("/fixture.ts") {
        source.decl_bodies();
    }
}

fn main() {}
