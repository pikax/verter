use super::*;
use verter_semantic::analysis::MacroFieldAddressTable;
use verter_session_query::analysis::top_level_owners::TopLevelOwnerTable;

#[test]
fn function_index_readers_keep_storage_charged_across_memo_teardown() {
    for memo_first in [true, false] {
        let context = PinContext::new(verter_retention::SemanticRetentionAccount::new(
            verter_retention::RetentionLimits::defaults(),
        ));
        let _scope = context.enter_for_test();
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
