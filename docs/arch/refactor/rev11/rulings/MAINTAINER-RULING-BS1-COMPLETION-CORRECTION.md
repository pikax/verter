---
ruling_id: "BS1-COMPLETION-CORRECTION"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["BS1"]
source_file: "MAINTAINER-RULING-BS1-COMPLETION-CORRECTION.md"
summary: "Extends MAINTAINER-RULING-BS1-COMPLETION-AUTHORITY (states explicitly it supersedes nothing). BS1-COMPLETION-PACKET.md accepted as authoritative gap-analysis but NOT ratified as the completion contract as-is. Corrects the charter's BF3-removal premise to zero eligible removals (BS1 instead retires seven BS0-authored #[ignore] guards). FC-HYDRATION-001 and FC-PERF-001 ruled BLOCKED/UNPROVEN, explicitly not N/A. Requires a standalone gate correction (real offline Svelte-oracle prerequisite probe, fail-loud on missing/invalid cache, enable bf2-authoritative in the canonical archive, prove the 45 additional tests execute) landed and reviewed independently before further BS1 completion evidence, followed by a byte-identity-proven rebase, a compound-ownership scoping ruling for SVELTE-MODULE, and an independently-reviewed performance-lock bootstrap with no threshold derived from BS1's own candidate results."
supersedes: []
superseded_by: []
contradicts: []
notes: "Its required gate correction (§5+§10) is what lands as 9275f0e40 per EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md, which discharges this ruling's §6 (rebase + byte-identity proof) but explicitly states the adversarial verdict does NOT carry across the fix-round rewrite — a fresh adversarial pass is owed, which ATTESTATION-BS1-ADVERSARIAL-EXACT-CANDIDATE.md then provides."
---

# Maintainer ruling — BS1 completion correction

**Date:** 2026-08-20. Supersedes nothing; extends MAINTAINER-RULING-BS1-COMPLETION-AUTHORITY.

1. `BS1-COMPLETION-PACKET.md` is ACCEPTED as the authoritative gap-analysis record, but NOT ratified as
   the completion contract in its current form. BS1 stays `IN_PROGRESS`; B5 stays `LOCKED`.
2. The seven completed Svelte client corrections and their existing reviews are RETAINED. No revert or
   restart authorized.
3. **Charter's BF3-removal premise corrected: zero BF3 guards remain eligible for removal.** BS1 instead
   owns retirement of the seven exact BS0-authored deferred-defect `#[ignore]` guards the packet
   identified. The two stale conformance records may be regenerated **from the pinned official oracle —
   NOT from candidate output** — and independently verified.
4. **FC-HYDRATION-001 and FC-PERF-001 are BLOCKED/UNPROVEN, not N/A.** Absence of Svelte server codegen
   means the server/hydration outcome is NOT YET IMPLEMENTED; absence of BS1 performance cells means the
   performance contract was NEVER MADE EXECUTABLE. The goal is to preserve the plan, so neither
   requirement is silently narrowed away. Excluding or transferring either requires a separate formal
   rescope with a named successor and DAG consequences.
5. **Before further BS1 completion evidence, land a standalone program-infrastructure correction to the
   canonical gate:**
   - a REAL offline Svelte-oracle prerequisite probe, run BEFORE Cargo;
   - when the cache is absent or invalid: fail SETUP loudly with the exact provisioning command; never
     inject a comparison note, never silently skip;
   - enable `verter_session/bf2-authoritative` in the canonical archive configuration;
   - prove the additional 45 tests are PRESENT and EXECUTE, including the official Svelte conformance
     tests;
   - add discriminating gate self-tests for missing, corrupt, and valid caches, and for accidental
     feature removal.
   The networked cache-provisioning script remains an explicit setup/CI step, never something tests
   invoke implicitly.
6. Land and review that gate correction INDEPENDENTLY; then rebase BS1 and MECHANICALLY prove the
   seven-fix code diff remains byte-identical.
7. Resolve the one compound-ownership capability row (`SVELTE-MODULE`, `owner = B3+BS1`) by an explicit
   SOURCE-BASED scoping ruling before ratifying the final completion contract.
8. Create the missing BS1 performance lock through an independently reviewed pre-measurement/bootstrap
   procedure. **No threshold may be derived from BS1 candidate results.**
9. Only once the corrected contract is self-contained are its EXACT BYTES ratified. The 7 PROVEN rows
   remain reusable; the 8 UNPROVEN rows remain BS1 work. Final acceptance still requires all three
   mandates bound to ONE exact final candidate.
10. Add the conformance crates and `verter_compiler` to `gate.mjs` Surface 3 `SHIPPED_CFG_FILTER`.

## Execution order derived from the ruling

§5 + §10 (gate correction) → independent review → §3 record regeneration from the pinned oracle →
§7 scoping ruling → §8 performance lock bootstrap → packet correction for §4 → §6 rebase + byte-identity
proof → §9 ratification → three mandates on one exact final candidate → acceptance.
