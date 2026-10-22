# Macro field-payload addressing inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclBodyMemo::macro_field_addresses` (the retained snapshot's `MacroFieldAddressTable`: content-free macro/field spans and member/property ordinals) | Every macro field-payload deref reads its node position here instead of replaying the file's macro assembly per field | REQUIRED | One `DeclBodyMemo` (one file version's retained snapshot); built on the first field-payload demand | `src/decl_body_memo.rs` (table type in `verter_semantic` `src/analysis/macros.rs`) | always |
| `DeclLoweringCounters::macro_field_address_builds` and `macro_field_address_work` | none; the wide-props growth test and the provenance snapshot | OPTIONAL | Per host; reset only by the provenance facade | `src/decl_lowering.rs` | `cfg(feature = "semantic-observe")` (`ObservedU64` fields) |
| `MacroFieldAddressTable::work` | none; aggregates address-build work for the provenance counter | OPTIONAL | One retained snapshot | `verter_semantic/src/analysis/macros.rs` | `cfg(feature = "semantic-observe")` (`ObservedU64`; storage and reader absent otherwise) |
