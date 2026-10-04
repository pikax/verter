use verter_session::for_tests::IndexedInputs;

fn cannot_escape<P: IndexedInputs + ?Sized>(port: &P) {
    if let Some(record) = port.shallow_file_state("/fixture.ts") {
        record.decl_bodies();
    }
}

fn main() {}
