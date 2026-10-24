use super::*;
use verter_semantic::analysis::MacroFieldAddressTable;
use verter_session_query::analysis::top_level_owners::TopLevelOwnerTable;

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
