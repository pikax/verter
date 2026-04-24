# Component-meta audit specs

Declarative validator specs for the six curated corpus representatives
(Accordion, Alert, App, AuthForm, Avatar, AvatarGroup). Plan §3
Commit 10 / F8.

Each spec is a JSON file consumed by
[`packages/benchmark/src/audit-validator.ts`](../../src/audit-validator.ts).
The legacy regex-validator (`trace-check.ts` + `trace-specs/component-meta/`)
is retired — these specs read directly from `RustAuditRecord` emitted
by the Rust-first native audit (plan §3 Commit 8).

## Fields

See `AuditSpec` in `audit-validator.ts` for the full field list:

- `requireLoadedFiles` / `forbidLoadedFiles` — set-based loaded-files
  assertions against `footprint.loaded_files()`.
- `requireInstantiations` / `forbidInstantiations` — identity matches
  against `footprint.instantiations`.
- `maxCounts` — per-record-kind caps (`instantiations`, `projections`, …).
- `maxDurations` — subject-substring caps against
  `footprint.materializations[*].duration_ms`.
- `totalDurationMsMax` — cap on `timings.total_ms`.
- `expectedResult` — minimum-shape assertions on the accompanying
  `ComponentMetaAnalysis` (`minProps`, `minEvents`, `minSlots`,
  `hasEvaluatedTypes`).
- `expectedFootprintSnapshot` — pinned JSON of the normalized
  footprint shape. Regenerate on intentional behavior changes.

## Usage

```ts
import specJson from "../audit-specs/component-meta/Accordion.json" with {
  type: "json",
};
import { validateAuditBundle } from "../src/audit-validator.js";

const result = validateAuditBundle(bundle, specJson);
if (!result.passed) {
  console.error(result.violations.join("\n"));
  process.exit(1);
}
```
