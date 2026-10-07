# Declaration header key index inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `MemberHeaderList` ordered members and key → position index (`TypeDeclHeader.member_headers`, `ValueDeclHeader.object_member_headers`) | Header walk first-wins / last-wins member dedup; member-presence facts, parse-stable hash and global-contributor readers of the ordered members; keyed member lookup | REQUIRED | Built once per header walk; retained with its `DeclHeaderIndex` (the file's `IndexedReady` artifact) and retired with it | `src/declarations/header_index.rs` | always |
| `MemberHeaderList::key_probes` key-index operation counter | none; linear key-work tests and measurement only | OPTIONAL | Per list; accumulates while the header walk builds it, never reset | `src/declarations/header_index.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `semantic-observe` forwarding to `verter_session_query/semantic-observe` | none; measurement builds only | OPTIONAL | Cargo feature selection | `Cargo.toml` | `cfg(feature = "semantic-observe")` |
