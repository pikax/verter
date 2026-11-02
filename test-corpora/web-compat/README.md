# Web compatibility corpus

| Path | Contents |
| --- | --- |
| `pinned/compat-snapshot.jsonl` | The admitted snapshot. Generated; do not edit |
| `pinned/NOTICE.md`, `pinned/LICENSE.*` | Attribution, modifications and upstream license texts |
| `schemas/` | The published upstream schemas, byte for byte |
| `fixtures/upstream-*.json` | Small upstream-shaped inputs covering every support condition |
| `fixtures/compat-snapshot.fixture.jsonl` | Those inputs through the same builder. Generated |

Regenerate with `node scripts/web-compat/generate.mjs`. The rules, and the steps
for a dataset upgrade, are in
[the ingestion contract](../../tests/web-compat/WBC1/contract.md).
