use super::*;
use verter_semantic::analysis::MacroFieldAddressTable;
use verter_session_query::analysis::top_level_owners::TopLevelOwnerTable;

#[test]
fn shallow_lens_readers_keep_storage_charged_across_memo_teardown() {
    lens_reader_keeps_storage_charged(DeclBodyMemo::shallow_lens);
}

#[test]
fn route_lens_readers_keep_storage_charged_across_memo_teardown() {
    lens_reader_keeps_storage_charged(DeclBodyMemo::route_fact_lens);
}

fn lens_reader_keeps_storage_charged<T: HeapSize + Send + Sync + 'static>(
    read: impl Fn(&DeclBodyMemo) -> Arc<T>,
) {
    let context = PinContext::new(verter_retention::SemanticRetentionAccount::new(
        verter_retention::RetentionLimits::defaults(),
    ));
    let _scope = context.enter_for_test();
    let memo = DeclBodyMemo::seeded_from_env(
        SnapshotKey {
            canonical: Arc::from("/retained-lenses.ts"),
            whole_hash: [34; 16],
            parse_env_hash: [0; 16],
        },
        &EvalEnv::default(),
        Arc::new(DeclHeaderIndex::default()),
    );
    let symbols = (0..128)
        .map(|n| DeclBindingKey::new(TopLevelOwnerId::ordinary_file(), format!("name{n}")))
        .collect::<FxHashSet<_>>();
    memo.install_shallow_lens(Arc::new(ShallowLens::new(
        symbols.clone(),
        symbols.clone(),
        symbols.clone(),
        FxHashMap::default(),
        FxHashMap::default(),
    )));
    memo.install_route_fact_lens(Arc::new(RouteLens::new(
        Arc::from("/retained-lenses.ts"),
        symbols,
        FxHashMap::default(),
    )));
    let reader = read(&memo);
    let charged = context.account().snapshot().pinned_bytes;
    assert!(Arc::ptr_eq(&reader, &read(&memo)));
    assert_eq!(context.account().snapshot().pinned_bytes, charged);
    drop(memo);
    let mut handles = verter_retention::allocation::AliasSet::estimating();
    let bytes = reader.heap_size(&mut verter_retention::heap_size::HeapMeter::new(
        &mut handles,
    ));
    assert!(bytes > 128 * std::mem::size_of::<DeclBindingKey>());
    assert!(
        context.account().snapshot().pinned_bytes >= bytes,
        "the escaping lens remains fully charged after memo teardown"
    );
    drop(handles);
    drop(reader);
    assert_eq!(context.account().snapshot().pinned_bytes, 0);
}

fn function_memo(context: &PinContext) -> (DeclBodyMemo, Arc<DeclLoweringService>) {
    let source: Arc<str> = Arc::from(
        (0..128)
            .map(|n| format!("export function f{n}(value: number) {{ return value + {n}; }}\n"))
            .collect::<String>(),
    );
    let service = Arc::new(DeclLoweringService::new_with_account(
        false,
        1,
        Arc::clone(context.account()),
    ));
    let memo = DeclBodyMemo::new(
        SnapshotKey {
            canonical: Arc::from("/retained-functions.ts"),
            whole_hash: [32; 16],
            parse_env_hash: [0; 16],
        },
        source,
        None,
        oxc_span::SourceType::ts(),
        Arc::new(TopLevelOwnerTable::ordinary_file(128)),
        false,
        Arc::clone(&service),
        Arc::new(DeclHeaderIndex::default()),
        Arc::default(),
        None,
    );
    (memo, service)
}

#[test]
fn whole_env_readers_keep_storage_charged_across_memo_teardown() {
    for memo_first in [true, false] {
        let context = PinContext::new(verter_retention::SemanticRetentionAccount::new(
            verter_retention::RetentionLimits::defaults(),
        ));
        let _scope = context.enter_for_test();
        let (memo, service) = function_memo(&context);
        let reader = memo.whole_env();
        assert_eq!(reader.total_decl_count(), 128);
        let mut handles = verter_retention::allocation::AliasSet::estimating();
        let bytes = reader.heap_size(&mut verter_retention::heap_size::HeapMeter::new(
            &mut handles,
        ));
        drop(handles);
        assert!(
            bytes > 128 * 1024,
            "fixture retains nonempty declaration tables"
        );
        let charged = context.account().snapshot().pinned_bytes;
        let second = memo.whole_env();
        assert!(Arc::ptr_eq(&reader, &second));
        assert_eq!(context.account().snapshot().pinned_bytes, charged);
        drop(second);
        if memo_first {
            drop(memo);
            drop(service);
            assert_eq!(reader.total_decl_count(), 128);
            assert!(
                context.account().snapshot().pinned_bytes >= bytes,
                "the escaping environment remains fully charged after memo teardown"
            );
            drop(reader);
        } else {
            drop(reader);
            assert_eq!(context.account().snapshot().pinned_bytes, charged);
            drop(memo);
            drop(service);
        }
        assert_eq!(context.account().snapshot().pinned_bytes, 0);
    }
}

#[test]
fn function_index_readers_keep_storage_charged_across_memo_teardown() {
    for memo_first in [true, false] {
        let context = PinContext::new(verter_retention::SemanticRetentionAccount::new(
            verter_retention::RetentionLimits::defaults(),
        ));
        let _scope = context.enter_for_test();
        let (memo, service) = function_memo(&context);
        let reader = memo.function_program_index().value;
        assert_eq!(reader.len(), 128);
        let mut handles = verter_retention::allocation::AliasSet::estimating();
        let bytes = reader.heap_size(&mut verter_retention::heap_size::HeapMeter::new(
            &mut handles,
        ));
        drop(handles);
        assert!(bytes > 128 * 1024, "fixture retains nonempty index tables");
        let charged = context.account().snapshot().pinned_bytes;
        let second = memo.function_program_index().value;
        assert!(Arc::ptr_eq(&reader, &second));
        assert_eq!(context.account().snapshot().pinned_bytes, charged);
        drop(second);
        if memo_first {
            drop(memo);
            drop(service);
            assert_eq!(reader.len(), 128);
            assert!(
                context.account().snapshot().pinned_bytes >= bytes,
                "the escaping index remains fully charged after memo teardown"
            );
            drop(reader);
        } else {
            drop(reader);
            assert_eq!(context.account().snapshot().pinned_bytes, charged);
            drop(memo);
            drop(service);
        }
        assert_eq!(context.account().snapshot().pinned_bytes, 0);
    }
}

#[test]
fn macro_field_address_storage_is_charged_once_until_the_last_cell_owner_drops() {
    let fields = (0..128)
        .map(|index| format!("p{index}: string;"))
        .collect::<String>();
    let source: Arc<str> = Arc::from(format!("defineProps<{{ {fields} }}>();"));
    let key = SnapshotKey {
        canonical: Arc::from("/macro-addresses.ts"),
        whole_hash: [31; 16],
        parse_env_hash: [0; 16],
    };
    let service = Arc::new(DeclLoweringService::new());
    let (_lease, table) =
        service.acquire_lease_and_run(&key, &source, oxc_span::SourceType::ts(), |program| {
            let program = program.expect("retained program");
            MacroFieldAddressTable::build(
                program.borrow_dependent(),
                program.source_str(),
                &TopLevelOwnerTable::ordinary_file(1),
                &verter_audit::observe::ObservedU64::default(),
            )
        });
    let table = table.expect("address table");
    let context = PinContext::new(verter_retention::SemanticRetentionAccount::new(
        verter_retention::RetentionLimits::defaults(),
    ));
    let _scope = context.enter_for_test();
    let memo = DeclBodyMemo::seeded_from_env(
        key,
        &EvalEnv::default(),
        Arc::new(DeclHeaderIndex::default()),
    );
    memo.pin_storage(&context);
    let cold = context.account().snapshot().pinned_bytes;
    let cell = Arc::clone(&memo.macro_field_addresses);
    let table = cell.get_or_init(|| Arc::new(table));
    let warm = context.account().snapshot().pinned_bytes;
    assert!(warm >= cold + 128 * std::mem::size_of::<verter_span::Span>());
    assert!(Arc::ptr_eq(table, cell.get().expect("cached addresses")));
    assert_eq!(context.account().snapshot().pinned_bytes, warm);
    drop(memo);
    assert!(context.account().snapshot().pinned_bytes > 0);
    drop(cell);
    assert_eq!(context.account().snapshot().pinned_bytes, 0);
}

#[test]
fn raw_surface_readers_keep_storage_charged_across_memo_teardown() {
    let fields = (0..128)
        .map(|n| format!("  field{n}: string;\n"))
        .collect::<String>();
    let source: Arc<str> = Arc::from(format!("export interface Kept {{\n{fields}}}\n"));
    let header_index = {
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&allocator, &source, oxc_span::SourceType::ts())
                .parse();
        assert!(!parsed.fatal_error);
        Arc::new(
            verter_semantic::analysis::script_shallow_index::build_script_shallow_index_with_owners(
                &parsed.program,
                &source,
                &TopLevelOwnerTable::ordinary_file(parsed.program.body.len()),
            )
            .expect("ordinary owners cover every statement")
            .declaration_headers,
        )
    };
    for memo_first in [true, false] {
        let context = PinContext::new(verter_retention::SemanticRetentionAccount::new(
            verter_retention::RetentionLimits::defaults(),
        ));
        let _scope = context.enter_for_test();
        let service = Arc::new(DeclLoweringService::new_with_account(
            false,
            1,
            Arc::clone(context.account()),
        ));
        let memo = DeclBodyMemo::new(
            SnapshotKey {
                canonical: Arc::from("/retained-surfaces.ts"),
                whole_hash: [35; 16],
                parse_env_hash: [0; 16],
            },
            Arc::clone(&source),
            None,
            oxc_span::SourceType::ts(),
            Arc::new(TopLevelOwnerTable::ordinary_file(1)),
            false,
            Arc::clone(&service),
            Arc::clone(&header_index),
            Arc::default(),
            None,
        );
        let reader = memo.raw_surfaces_for("Kept", SymbolSpace::Type);
        assert_eq!(reader.len(), 1);
        let mut handles = verter_retention::allocation::AliasSet::estimating();
        let bytes = reader.heap_size(&mut verter_retention::heap_size::HeapMeter::new(
            &mut handles,
        ));
        drop(handles);
        assert!(bytes > 0, "fixture captures a nonempty surface");
        let charged = context.account().snapshot().pinned_bytes;
        let second = memo.raw_surfaces_for("Kept", SymbolSpace::Type);
        assert!(Arc::ptr_eq(&reader, &second));
        assert_eq!(context.account().snapshot().pinned_bytes, charged);
        drop(second);
        if memo_first {
            drop(memo);
            drop(service);
            assert_eq!(reader.len(), 1);
            assert!(
                context.account().snapshot().pinned_bytes >= bytes,
                "the escaping surfaces remain fully charged after memo teardown"
            );
            drop(reader);
        } else {
            drop(reader);
            assert_eq!(context.account().snapshot().pinned_bytes, charged);
            drop(memo);
            drop(service);
        }
        assert_eq!(context.account().snapshot().pinned_bytes, 0);
    }
}
