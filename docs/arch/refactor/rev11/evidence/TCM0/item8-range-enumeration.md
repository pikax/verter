# `charters/TCM3.md` item 8's range, enumerated

Item 8 is a **general conditional**: *"If the official API lacks a required capability, the legal
outcomes are exhaustive: reassign to direct-LSP ownership, implement natively in Verter, require and
certify an upstream API addition/fix, or keep TCM4 blocked."*

A ruling that settles it for one capability leaves it live for every other in its range, while reading
as though it closed the question. **So the range is enumerated here and every member gets a stated
disposition.** This file makes no determination — it enumerates, cites the acts that decided, and marks
what is undecided as undecided.

## The range, and how it was bounded

Item 8 governs a capability the **official API lacks**. The candidate's limitations are recorded in one
place: `package-lock-and-semantic-api.md` §4e and §6.2(a)-(e). §6.3 bounds the set from the other side by
enumerating what the probes confirm **works** — project/source lookup, all eight diagnostic getters, the
`Checker`'s single-value symbol/type operations, the bulk array overloads, declaration and JS emit. A
capability in §6.3 is supplied and item 8 cannot reach it.

**Derivation of the range: §4e ∪ §6.2(a)-(e) — six members.** Falsifier: an API limitation recorded
outside those sections. A sweep for absence-phrasing across the document returns one further hit
(`:267`), which is an argument about the `APIMethodInfo` table rather than a recorded absence.

## Every member, with its disposition

| # | capability / limitation | is it an ABSENCE item 8 governs? | disposition |
|---|---|---|---|
| §4e | **cancellation** — zero hits for `cancel`/`Cancel`/`AbortSignal` across both API surfaces; the async surface differs only by `Promise<T>` | **YES** — a genuine absence with **no composition** | **DISPOSED BY CHARTER.** `charters/TCM3.md` item 5 requires TCM3 to design its own in-flight-query abandonment and states it "is NOT an open question TCM3 may leave unresolved". Never reaches item 8 |
| §6.2(b) | **project-wide find-all-references** — the only primitive is file-local and the cross-file failure mode is a silent empty result | absence of a **single call**, not of the capability | **TRIGGER DOES NOT FIRE**, ruled. The probe names the composition: enumerate candidate files, resolve each file's own alias symbol, union |
| §6.2(c) | **auto-import completions** — `getCompletionsAtPosition` rejects any list needing them, "not obtainable from this API's completion call at all" | absence of a **single call**, not of the capability | **TRIGGER DOES NOT FIRE** — act `ce637bcf7`, which is content-pinned to `package-lock-and-semantic-api.md`, the document whose bytes carry the composition it rests on. (An earlier revision of this cell said the act pinned THIS document; it does not.) What it decides is the registry's to state and is not repeated here. An earlier revision of this row said "ruled" before any act existed; the act now exists and was read out of the tree before being cited |
| §6.2(a) | **diagnostic wire shape** — `{fileName?, pos, end, code, category, text, …}` rather than `start`/`length`/`messageText` | **NO.** The API delivers diagnostics; the field names differ | Not in range. A caller-side adaptation constraint, and the evidence records it as one. The hazard it carries is real and separate: code written to the classic shape reads `undefined` **silently** |
| §6.2(d) | **out-of-range position → recovered Go panic** rather than a typed rejection; the session survives | **NO.** The capability works; the failure MODE for invalid input is wrong | Not in range. The evidence's own conclusion is "clamp and validate on the Verter side" — a robustness constraint |
| §6.2(e) | **out-of-range `Checker` positions degrade to the file's module symbol** rather than `undefined` | **NO.** Same shape as (d) | Not in range. Evidence: "Same conclusion as (d): clamp and validate on the Verter side" |

**Result: six members, ZERO live.** One disposed by charter (§4e), **two ruled** non-triggering by composition (§6.2(b) by `f2ec18715`, §6.2(c) by `ce637bcf7`), three outside the range because they are not absences at all. Nothing in item 8's range is left for a later block to choose among.

**This line has been wrong twice, in opposite directions, and both are worth keeping.** It first read "zero live" on the strength of a ruling that had been communicated but never landed — the ledger carried the references ruling and nothing at all about auto-imports. An acceptance lane caught it and it was corrected to "one live". The act has since been issued and read out of the tree, so zero is now true for a checkable reason rather than a remembered one. **Being told a ruling exists and the ruling existing are different facts, and only the record distinguishes them.**

## The escape, attaching to both rulings

Both non-triggering dispositions rest on a composition each probe **demonstrated**, not asserted. **If
a later finding shows either composition does not deliver its capability, the trigger fires for that
capability and the choice returns** — already narrowed, for references, to a maintainer decision between
an upstream addition and keeping the dependent blocked, with direct ownership excluded because its only
implementation is the silent degradation item 8's own final sentence forbids, and native excluded on the
"needs TS: none" test. The same narrowing applies to auto-imports on the same reasoning.

A ruling that names its own falsifier stays safe to rely on; these do.

## What this file does not do

It makes no determination about any member. Where an act decided, the act is cited and its content is
left to the record that owns it. Where nothing has decided, the row says so.
