# Owner-local exact resolution publication inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_workspace

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ResolutionWorldRoot::exact_owners`: ordered persistent map, importer → that importer's exact bucket | Exact lookup in every resolution attempt (`ResolutionWorldRoot::exact`); subtree retirement of exact importers (`exact_owners_under`, one point lookup plus one range seek over the directory's descendants) | REQUIRED | Per immutable world root; a replacement root shares every bucket it did not replace. An importer leaves with its last route | `src/resolution_currency.rs` | always |
| `ExactOwnerBucket::routes`: one importer's routes keyed by `(raw specifier, phase, kind)`, last route per key wins | Exact lookup; the owner-local unchanged comparison and the exact fact keys a changed refresh advances (`replace_owner_exacts`) | REQUIRED | Immutable once published; held by every root that published it, dropped with the last one | `src/resolution_currency.rs` | always |
| `EdgeStore::exact_resolutions_unchanged` / `replace_exact_resolutions` unchanged gate, compared against the last-route-per-key table a replace would store (an owner with no edge state stores the empty table); a replace stores only those winning routes and their targets | Skips the edge write and the world publication for a refresh that would store the published table again; `Engine::set_exact_resolutions` decides it under the publication gate before opening a publication window, so an unchanged refresh builds no replacement root and never turns the epoch odd | REQUIRED | Per call | `src/exact_resolution.rs` | always |
| `EXACT_PUBLICATION_WORK` thread-local counter, `take_exact_publication_work` reader and `record_exact_publication_work` hook: exact routes compared, replaced or turned into fact keys, plus importers a subtree seek visited | none; tests and measurement builds only | OPTIONAL | Per thread; the reader resets it | `src/resolution_currency.rs` | `cfg(any(test, feature = "semantic-observe"))` |

A refresh reads and replaces only its importer's bucket, so a changed or
unchanged refresh, a parsed-edge refresh carrying the routes, and an
importer's deletion do the same exact-table work at any importer count. The
hook is added to once per bucket operation or seek, never once per entry.
Default builds compile the hook, its call sites and the counts they pass
away entirely; there is no per-operation enabled check.
