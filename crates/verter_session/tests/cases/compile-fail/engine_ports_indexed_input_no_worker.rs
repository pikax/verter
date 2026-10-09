use verter_type_engine::resolver_core::request_ports::IndexedInputs;

fn cannot_escape<P: IndexedInputs + ?Sized>(port: &P) {
    if let Some(record) = port.shallow_file_state("/fixture.ts") {
        record.decl_bodies();
    }
}

fn main() {}
