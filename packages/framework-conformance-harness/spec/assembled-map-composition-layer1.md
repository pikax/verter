# Assembled Vue main-module source-map composition — LAYER 1 (frozen semantic specification)

**Artifact:** `assembled-map-composition-layer1`
**Revision:** 8 (revisions 1–7 revised under successive independent review rounds; revision 7 was
adopted as frozen at blob `0ea47424acfbd4913e11f16156baa597216c84fb`. Revision 8 is a POST-FREEZE
amendment — see §12 — closing one narrow gap found while writing the independent reference and
production implementations against the frozen revision 7 text; revision 8 was itself independently
re-adopted at blob `085139c5267136ed0c2fa39d78ad48168c6e0e76`.)
**Status:** **REVISIONS 1–8 ARE ADOPTED.** Revision 8's sole change (§4.3 step 2.1's `fragment`
attribution for `U8.1`, registered as `DECISION` D-8) closed its own independent review (two rounds:
conformance FAIL → PASS, adversarial PASS_WITH_NOTES → PASS, converging on the same scoping
correction) and its own adoption record, distinct from the revision-7 freeze. Both the independent
reference and the production implementation are held to the reviewed D-8 wording as adopted.

**Specified against tree:** `program/architecture-lock` @ `20b03aaf1`. Every citation below is to
that tree. §11.5 states what happens when a cited implementation later changes.

---

## 1. Scope and status

### 1.1 What this document is

This is **layer 1** of the two-layer normative specification of the assembled Vue main-module
source-map composition algebra.

Layer 1 fixes:

- the **emission standard** (§2.6) — the single rule deciding whether any given segment is emitted at
  all, from which §5's and §6's emission rules are derived,
- the pre-assembly **input DTO schema**, including its string-domain precondition (§3),
- the **validation order** — a single total order over every sub-check — and the exhaustive
  **`UncomposableInputMap` rejection taxonomy** (§4),
- the **chaining/transform algebra** for both authorized rewrites, including equal-coordinate
  ordering, collision policy, and sourceless-barrier semantics (§5),
- an exhaustive **assembler write-site manifest**, given as an exact byte grammar, and the
  **boundary placement rules** for every transition between mapped fragment content and
  assembly-owned synthetic content (§6),
- the **canonical output artifact schema**: field presence/policy, table composition and index
  remapping, and the canonical `mappings` encoding (§7),
- **provenance** bookkeeping (§8).

§10 maps each requirement of AMD-008's umbrella description and the layer-2 seed artifact's own
`knownGaps` list to the section that answers it.

### 1.2 Relationship to layer 2 and to the charter

Layer 2 is the literal vector coverage set at
[`../vectors/assembled-map-composition.vectors.json`](../vectors/assembled-map-composition.vectors.json).
Per AMD-008:206-208, **where a vector and frozen layer 1 could be read to disagree, layer 1 governs
and the vector is the thing that is wrong.** §9 records exactly where the current SEED vectors and
this specification differ, with derivations.

Per AMD-008:161-164, after this document is frozen, changing layer-1 semantics requires its own
amendment. It is not a BV0A implementation decision, it is not something a vector can do, and it is
not something a change to a shared helper elsewhere in the tree can do (§11.5).

Where charter prose and this document could be read to disagree on a matter this document owns, the
frozen artifact governs (BV0A.md:37-39). This document does not amend the charter and does not
enlarge BV0A's scope.

### 1.3 Non-goals (inherited from BV0A's owned-scope exclusions)

This document specifies nothing about, and confers no authority over: B3's canonical request or
map-policy model; B4's logical source units, stable identity system, generic fragment contract,
source-space architecture, atomic artifact set, or publication transaction; BV1's Vue semantic
train; B5's direct compiler core; a universal or cross-framework IR; a Svelte path; IDE, TSC,
declaration, style-content, or custom-block mapping; a whole-module or cross-block chunk IR
(AMD-008:250-253); or any change to BF2's accepted authored-source mapping oracle or its invocation
(BV0A.md:123-126).

It specifies nothing about whether an authored `original` coordinate, source spelling, or name tells
the truth about the authored SFC. Those are carried **opaquely** (BV0A.md:19-22); their truthfulness
is BV0's concern.

### 1.4 Reviewer's note on method

Every factual claim about existing behaviour is cited to a file and line in the tree named above.
Claims that are **definitional choices** — questions the real sources leave open, which layer 1
exists to settle — are marked **`DECISION`** and carry the rationale, the strongest opposing
argument, and its resolution. §11 lists every `DECISION` in one place.

---

## 2. Model, coordinates, and accepted primitives

### 2.1 Coordinate model

- **Lines** are 0-based; **columns** are 0-based **UTF-16 code units**; both as on the wire.
- The **line table** of a text splits on `U+000A` (LF) only and **retains** any preceding `U+000D`
  (CR) inside the line's text: `lineTable(t) = t.split("\n")`. A text ending in LF therefore has a
  final, empty line. This is the accepted harness semantics (`src/mapping-oracle.mjs:71-73`) and
  matches the producer, whose column advance counts UTF-16 units from the last LF
  (`code_transform/source_map.rs:577-591`, `crates/verter_parser/src/cursor/position.rs:15-59`).
- A **position** `(line, column)` is **in-bounds** for a text iff
  `0 ≤ line < lineTable(t).length` and `0 ≤ column ≤ lineTable(t)[line].length`. A column equal to
  the line length is in-bounds and denotes end-of-line (`src/mapping-oracle.mjs:86-102`, the `eol`
  arm). An LF byte itself occupies **no column**: it is the separator, and belongs to no line's
  text. This fact is load-bearing in §6.4.
- Rust-side byte offsets and wire-side UTF-16 columns are **converted at the owning boundary** and
  never mixed as raw offsets (BV0A.md:90-92). §5.2 states the exact conversion.

### 2.2 Segment

A **segment** is the decoded record `{ genLine, genCol, srcIdx, srcLine, srcCol, nameIdx }`, with
all four authored fields `null` for a 1-field wire segment and `nameIdx` `null` for a 4-field wire
segment (`src/sourcemap.mjs:70-106`). A segment whose `srcIdx` is `null` is **sourceless**;
otherwise it is **source-bearing**. `(srcIdx, srcLine, srcCol, nameIdx)` is a segment's **payload**.

A **segment sequence** is an ordered list of segments. Order is load-bearing: several segments may
share one generated coordinate, the accepted decoder preserves their wire order
(`src/sourcemap.mjs:80-103`), and the accepted lookup selects the **last** applicable segment at or
before a column (`src/mapping-oracle.mjs:1039-1048`), so reordering two equal-coordinate segments
changes which authored position a consumer resolves (AMD-008:118-126).

### 2.3 The accepted lookup, `resolveAt`

`resolveAt(S, line, column)` over a segment sequence `S` is:

> Among the segments of `S` on `line`, ordered by `genCol` ascending and, **at equal `genCol`, by
> their order in `S`**, take the LAST whose `genCol ≤ column`. If there is none, the result is
> **absent**.

This is `src/mapping-oracle.mjs:1039-1048` applied to `segmentsByLine`
(`src/mapping-oracle.mjs:1166-1171`); the per-line sort is by `genCol` only and
`Array.prototype.sort` is stable, so equal-coordinate segments keep their sequence order.

Two properties are load-bearing throughout §5:

- **It is line-scoped.** A line with no applicable segment resolves to *absent*; the lookup does not
  fall through to a previous line.
- **It is not sourceless-transparent.** A sourceless segment is a legitimate result. The lookup
  never skips past a sourceless segment to find a nearer source-bearing one. This is the
  **sourceless barrier**: a region a producer deliberately left unmapped stays unmapped through
  composition.

### 2.4 `CodeTransform` token geometry (the normative rewrite primitive)

AMD-008:244-253 and BV0A.md:56-70 make `CodeTransform`'s **local** code-and-map semantics normative
for the two authorized rewrites. The facts this specification depends on:

| # | Fact | Evidence |
|---|---|---|
| CT-1 | A fresh transform over a non-empty source holds exactly one chunk, `Original{0, len}`; over an empty source it holds none. | `code_transform.rs:186-195` |
| CT-2 | `remove(s,e)` is exactly `overwrite(s,e,"")`. | `code_transform.rs:948-950` |
| CT-3 | `overwrite` with `start >= end` is a silent no-op; empty replacement content is the static `""`. | `code_transform.rs:676-685` |
| CT-4 | An `overwrite` splits the covering `Original` chunk at the edit boundaries only, producing at most `Original[cs,start)`, `Overwritten{start,end,content}`, `Original[end,ce)`; no empty `Original` chunk is ever produced, and chunk boundaries are therefore exactly `{0} ∪ {edit starts} ∪ {edit ends} ∪ {len}`. | `code_transform.rs:617-644`, `:834-853` |
| CT-5 | **When the map declares a source** (`SourceMapOptions.source` is `Some`, so `source_id` is `Some`), an `Original` chunk emits **unconditionally** one token at its own start, then one token at each interior boundary. With no declared source an `Original` chunk emits nothing at all. Every map this specification composes or models declares a source, so the qualified and unqualified readings coincide here. | `source_map.rs:181-197` (the `if let Some(source_id)` guard), `:510-517` |
| CT-6 | The interior boundaries of an `Original` chunk are its interior LF positions (`nl_index + 1`) filtered by `position < content_len`, merged with any registered `sourcemap_locations` strictly inside the chunk. A chunk-**terminal** LF therefore emits no token. | `source_map.rs:523-528`, `:531-569`; capacity mirror at `:459-461` |
| CT-7 | A registered `sourcemap_location` at exactly the chunk start is already covered by the chunk-start token and emits no second token; one at the chunk end belongs to the next chunk. | `source_map.rs:519-524` |
| CT-8 | **When the map declares a source** — the same `if let Some(source_id)` guard CT-5 carries — a **non-empty** `Overwritten` chunk emits exactly ONE token, at the replacement's generated start, mapped to the overwritten range's ORIGINAL start. There are no interior tokens. The generated position advances by the replacement either way, because the advance sits **outside** that guard. Both modelled passes declare a source (§5.1), so the guard is satisfied by construction; with no declared source a pass map would carry no tokens at all and would not be the configuration this specification models. | `source_map.rs:243-276` (guard at `:255`, advance at `:271-275`) |
| CT-9 | An **empty** `Overwritten` chunk emits NO token and advances the generated position by zero. | `source_map.rs:248-250` |
| CT-10 | An `Inserted` / `InsertedAnchored` chunk, a non-empty intro, and a non-empty outro each emit one **sourceless** token. Neither rewrite pass creates any of these. | `source_map.rs:152-160`, `:277-296`, `:367-375` |
| CT-11 | Source and generated positions inside an `Original` chunk advance in lockstep through the same content. | `source_map.rs:550-560` |
| CT-12 | Original-side line/column for a chunk or overwrite start is resolved by `offset_to_line_and_col`, which returns 1-based line and 1-based UTF-16 column, converted to 0-based at the emission site. | `source_map.rs:506-508`, `:256-259`; `crates/verter_parser/src/cursor/position.rs:144-164` |
| CT-13 | Neither `overwrite` nor `remove` registers any `sourcemap_location`; the sole registration API is the separate `try_add_sourcemap_location`. Both rewrite passes therefore have an empty location set. | `code_transform.rs:674-689`, `:789-940` (no push); the only push is `fallible.rs:338-345` |
| CT-14 | `build_string` concatenates intro, every chunk's bytes in chunk order, then outro. | `code_transform.rs:1304-1329` |

### 2.5 The two authorized rewrites, as they exist today

```rust
let mut script_code = script.code.clone();
script_code = script_code.replace("__sfc__", "_sfc_main");
script_code = script_code.replace("export default _sfc_main;\n", "");
```

(`crates/verter_session/src/compile.rs:82-85`.)

- **Pass 1** replaces every occurrence of the literal byte pattern `__sfc__` (7 bytes) with
  `_sfc_main` (9 bytes).
- **Pass 2** operates on **pass 1's output coordinate space** — its pattern contains the pass-1
  output spelling — and removes every occurrence of the literal byte pattern
  `export default _sfc_main;\n` (26 bytes).
- Both patterns are matched **literally, globally, non-overlapping, left-to-right**, exactly as
  `str::replace` does. Matching is **not** identifier-aware: `___sfc__` contains `__sfc__` at offset
  1 and is rewritten. This is existing pinned behaviour and the assembled code baseline is
  byte-pinned (BV0A.md:202-225), so the map algebra reproduces it rather than "improving" it.
- Both patterns are pure ASCII, so every match boundary is a UTF-8 character boundary (§5.2's
  boundary lemma).
- Both rewrites apply to the **script fragment only**. The template fragment's code is written
  verbatim (`compile.rs:178-188`) and is never rewritten (§5.7).
- The module's own trailing `export default _sfc_main` (`compile.rs:252`) carries no `;` and no
  trailing newline, so it is not an instance of pass 2's pattern.

### 2.6 The emission standard

Several rules below decide whether a segment is emitted at all. They are all decided by one standard,
stated once here and cited from each of them, so that adjacent geometry cannot end up governed by
two rules reaching different-looking answers for different-looking reasons:

**The observable.** `resolveAt` (§2.3) returns two distinguishable things where a consumer sees one:
*absent* (no applicable segment) and *a present sourceless segment* are different return values but the
same answer to the only question a source map is asked. Define the observable accordingly:

> `payloadAt(S, line, column)` is the **authored payload** the assembled map yields at a generated
> position: the `(srcIdx, srcLine, srcCol, nameIdx)` tuple of `resolveAt(S, line, column)` when that
> is a **source-bearing** segment, and the single distinguished value **`Unmapped`** in *both* other
> cases — when `resolveAt` is absent, and when it returns a sourceless segment.

That quotient is not introduced here; it is the semantics this document already runs on, and it is what
three independent things already say:

- **§5.4** makes a sourceless segment a legitimate *result* — "this position is unmapped" — rather than
  a hole to be seen through. A sourceless segment and no segment differ in what they *stop*, not in
  what they *say*.
- **BR-5 (§6.4)** is stated as "resolves to a **source-bearing** segment", so its own predicate already
  reads `payloadAt`, not `resolveAt`.
- **The accepted oracle** tests boundary inheritance as `inherited !== null && inherited.srcIdx !== null`
  (`src/mapping-oracle.mjs:1324-1325`) — one branch for both absent and sourceless.

The distinction `payloadAt` erases is still real and still load-bearing: a sourceless segment placed
*before* other segments on its line changes what `payloadAt` yields at columns to its right, which is
exactly how a barrier works (§5.4) and why BR-3 exists (§6.4). `payloadAt` erases the difference at one
position, not the difference a segment makes to its line.

**The standard.**

> **A composition rule emits a segment only if there exists an admissible input for which emitting it
> changes `payloadAt` at some generated position of the assembled module** — where *admissible input*
> means any DTO instance that passes §4's validation.
>
> The quantifier is over the **rule**, not over the instance. A rule that can matter for some
> admissible input fires for **every** input its syntactic condition selects; it is never conditioned
> on whether it happens to matter for the input at hand.

The two halves both do work. The existential half excludes segments that can never change any authored
payload anywhere: adding one is weight without meaning, and the charter's no-invent posture gives no
warrant for it (§11.2 applies this to exclude two `CodeTransform` token kinds). The universal half keeps
every surviving rule **syntactic** — a function of the fragment's bytes and the write grammar, never of
the composed segment content — so that two independent implementations cannot disagree about *when* to
evaluate a content predicate (§11.3 applies this to the boundary rule).

Applied to the whole specification, the standard admits exactly five kinds of emitted segment:

1. input segments carried inside a chunk (§5.3(a));
2. input segments carried at the fragment's end position (§5.3(d));
3. replacement segments (§5.3(b));
4. resume segments (§5.3(a));
5. fragment-end boundary segments (§6.4 BR-3).

Nothing else is emitted.

### 2.7 What the composition is, in one sentence

> The assembled map is the ordered concatenation, in assembly write order, of each mapped fragment's
> segment sequence after that fragment's own map has been chained through its rewrite passes and
> translated by the fragment's placement, with assembly-owned bytes contributing no segments other
> than the fragment-end boundary segments §6.4 requires.

---

## 3. Pre-assembly input DTO

### 3.1 What the DTO must be

Per AMD-008:226-230 the independent reference consumes **one serialized pre-assembly input DTO
carrying every input the real assembler reads and nothing else** — never the production map, splice
lists, placement traces, or composition helpers.

The DTO is the input surface of the assembler **under this specification** — the assembler that
returns code *and* map. Its fields are partitioned into **(C)** code-determining inputs, each one an
input the current, unmodified `assemble_vue_main_module` reads, cited to the line that reads it, and
**(M)** map-determining inputs, each added by BV0A and justified by a ratified requirement.

Nothing else is admissible. In particular the DTO carries **no** style or custom-block content, **no**
style map, **no** `compiled.main` field, **no** `compiled.inline` flag, and no placement, offset,
splice, or cursor information of any kind — placement is **derived** as the assembler writes (§6.3),
never supplied.

The DTO's transport encoding is JSON. Its identity is the field list of §3.3.

### 3.2 The real assembler's signature and reads

```rust
pub fn assemble_vue_main_module(
    canonical_id: &str,
    compiled: &RuntimeCompileOutput,
    meta: &FileMeta,
    profile: &CompileProfile,
) -> String
```

(`compile.rs:70-75`.) The body reads exactly the following, and nothing else:

| Read | Site |
|---|---|
| `canonical_id` | `compile.rs:103`, `:108` (via `render_ids`), `:208` (`__file`), `:242` (SSR fallback id) |
| `compiled.styles.len()` | `compile.rs:102`, `:112` |
| `compiled.custom_blocks.len()` | `compile.rs:107`, `:112`, `:199` |
| `compiled.template.imports` | `compile.rs:154-165` |
| `compiled.template.ssr_imports` | `compile.rs:167-176` |
| `compiled.template.code` | `compile.rs:178-196` (written; also text-scanned at `:192`, `:194`) |
| `compiled.script.code` | `compile.rs:94` (read), `:126-146` (written) |
| `compiled.scope_id` | `compile.rs:149-150` (no-script branch only) |
| `meta.style_langs[idx]` | `id.rs:201-215`, reached from `compile.rs:103` |
| `meta.custom_types[idx]` | `id.rs:216-226`, reached from `compile.rs:108` |
| `profile.runtime_module_name` | `compile.rs:156`, `:233` |
| `profile.is_production` | `compile.rs:207`, `:211` |
| `profile.ssr` | `compile.rs:211`, `:232` |
| `profile.hmr_strategy` | `compile.rs:212` |
| `profile.ssr_module_id` | `compile.rs:242` |

No other field of `RuntimeCompileOutput` (`carrier_compiler.rs:601-642`), `FileMeta`
(`types.rs:2523-2541`) or `CompileProfile` (`types.rs:1322-1397`) is read.

### 3.3 The DTO schema

```
AssembleInput := {
  canonicalId          : string                       // (C)
  styleCount           : uint32                       // (C)
  customBlockCount     : uint32                       // (C)
  styleLangs           : array<string|null>           // (C)
  customTypes          : array<string>                // (C)
  script               : ScriptFragment | null        // (C)+(M)
  template             : TemplateFragment | null      // (C)+(M)
  scopeId              : string                       // (C)  "" = none
  runtimeModuleName    : string | null                // (C)  null ⇒ "vue"
  isProduction         : boolean                      // (C)
  ssr                  : boolean                      // (C)
  ssrModuleId          : string | null                // (C)
  emitSsrModuleRegistration : boolean                 // (C)  meaningful only when ssr
  hmrStrategy          : "vite" | "webpack" | "none"  // (C)
  sourceMapRequested   : boolean                      // (M)
  authored             : { script: boolean, template: boolean }   // (M)
}

ScriptFragment := {
  code       : string                                 // (C)
  sourceMap  : string                                 // (M)  "" = no map; otherwise RAW, unparsed
}

TemplateFragment := {
  code       : string                                 // (C)
  imports    : array<string>                          // (C)
  ssrImports : array<string>                          // (C)
  sourceMap  : string                                 // (M)  "" = no map; otherwise RAW, unparsed
}
```

Field notes:

1. **`styleCount` / `customBlockCount` are separate from `styleLangs` / `customTypes`.** The
   assembler iterates `0..compiled.styles.len()` and `0..compiled.custom_blocks.len()`
   (`compile.rs:102`, `:107`, `:199`), while `render_ids` indexes `meta.style_langs` and
   `meta.custom_types` with `.get(idx)` and falls back to `"css"` / `"custom"` when the index is
   absent (`id.rs:202-207`, `:217-221`). The two lengths can legitimately differ and the emitted
   bytes depend on both.
2. **The fragment maps are RAW, unparsed strings.** They are `String` on the producer side
   (`carrier_compiler.rs:525-526`, `:542-543`), with `""` meaning "no map", as the caller already
   treats them (`virtual_file_pipeline.rs:3056-3060`). Passing them raw is required so that the
   malformed-JSON rejections of §4.4 are exercisable at all.
3. **`sourceMapRequested` is `CompileProfile::source_map`** (`types.rs:1364-1365`). The current
   assembler does not read it; BV0A's must, because "no map was requested" and "a required map is
   missing" are different outcomes (§4.2) and must not be conflated with the emptiness of a
   fragment's `sourceMap` string.
4. **`authored` is the pre-assembly authored-fragment inventory.** `authored.script` is
   `FileMeta::has_script`, set from the presence of a `<script>` / `<script setup>` block
   (`parse.rs:1302-1310`); `authored.template` is `FileMeta::has_template`, set **independently**
   from `parsed.template_ast()` (`parse.rs:1347-1355`). Both reach `FileMeta` at `parse.rs:1577-1579`.
   AMD-008 requires map-requiredness to come from this inventory and **never** from
   `compiled.script.is_some()` (AMD-008:365-369, BV0A.md:263-266).
5. **`hmrStrategy` is the three-variant enum** `HmrStrategy` (`types.rs:75`), not a free string.
6. **`scopeId`** is `RuntimeCompileOutput::scope_id` (`carrier_compiler.rs:615-616`); `""` means none.
7. **No placement field.** Placement is derived (§6.3). A DTO that supplied it would let an
   implementation pass while its write grammar and its map disagree.

### 3.4 Map requiredness

Let `authored.script` / `authored.template` be the inventory and `script != null` / `template != null`
be presence in the bundle.

> A fragment's map is **REQUIRED** iff the fragment is **both authored and present**. A required map
> is present iff its `sourceMap` string is non-empty.

- **Present but not authored** — the compiler-synthesised script block of a template-only cell — is
  not required to carry a map (BV0A.md:263-266; seed vector F7).
- **Authored but not present** — the inline topology, where `meta.has_template` is `true` while
  `compiled.template` is `None` because the render closure lives inside `setup()`
  (`compile.rs:596-607` exhibits exactly this combination) — requires nothing, because the fragment
  contributes no bytes.

AMD-008's prohibition is on deriving requiredness *from presence alone*, not on presence
participating at all; the alternative would demand a map for a fragment that emits no bytes.

When `sourceMapRequested` is `false`, no map is produced (§7.7) and no fragment map is required; a
non-empty `sourceMap` string is ignored.

### 3.5 DTO string-domain precondition

Six DTO strings are embedded verbatim or Debug-quoted into the assembled module's bytes:
`canonicalId`, `ssrModuleId`, `scopeId`, `runtimeModuleName`, each `styleLangs[i]`, and each
`customTypes[i]` (§6.2). Two of the embedding sites use Rust's `{:?}` `str` formatting
(`compile.rs:208`, `:245`), whose escaping beyond the five two-character escapes is driven by the
standard library's Unicode printability and grapheme-extended tables — a table this specification
deliberately does not reproduce, because reproducing it in a second language is a
Unicode-version-coupled hazard, not a semantic decision.

> **Precondition P1.** Every one of those six strings consists solely of characters in
> `U+0020`–`U+007E` (printable ASCII).

Under P1 the two formatting functions of §6.2 are exactly reimplementable in any language, and the
write grammar of §6.2 is an exact byte grammar. **This specification defines no behaviour for a DTO
instance violating P1**; such an instance is a malformed test input, not a composition outcome
(§11.4). P1 is satisfied by every canonical id, scope id, runtime module name, style lang and custom
block type in BF2's 36-cell seed manifest, so it does not bound BV0A's exit; extending the
specification beyond P1 is a future amendment's work, or B4's.

`code` and `sourceMap` are **not** subject to P1: fragment code is copied byte-for-byte and never
escaped, and a map string is parsed, not embedded. Astral and CRLF fragment content is fully in
scope (and is exercised by seed vectors V5 and V6).

---

## 4. Validation: order and the `UncomposableInputMap` taxonomy

### 4.1 Contract

Missing, malformed, ambiguous or uncomposable required input mapping is a hard fail-closed outcome —
never coerced into an empty, approximate, or unmapped successful result (BV0A.md:176-181, :251-259;
AMD-008:332-350). Validation runs **to completion before any composition work begins**: no segment is
translated, no table row is appended and no artifact field is computed until §4.3's steps have all
passed.

Every rejection reports **exactly one** outcome. §4.3 is a **total order over every individual
check**, including the element order within each scanned array and the field order within one
segment, so any input for which two or more conditions hold has one determined reported outcome. The
first failing check in that order is the outcome; validation stops there.

### 4.2 The two fail-closed outcome kinds

| Outcome | Meaning |
|---|---|
| `MissingRequiredInputMap{ fragment }` | A fragment whose map is REQUIRED (§3.4) carries an empty `sourceMap`. Not a member of the eight `UncomposableInputMap` families: the charter treats a missing map and an uncomposable map as separate triggers (BV0A.md:176-181 vs :251-259). |
| `UncomposableInputMap{ family, code, fragment }` | A present map is structurally unusable. `family` is one of the eight ratified families U1–U8 (BV0A.md:255-259); `code` is the exact sub-code of §4.4. |

Both are hard failures. **Neither returns a partial result: on either outcome the assembler produces
no successful result at all — not code without a map, not code with an empty map.** Returning the
assembled code while reporting a map failure would be exactly the "unmapped successful result" the
charter forbids (BV0A.md:251-254), and the two exits that consume this path require code and map
together (BV0A.md:212-219). Rescope routing (to BF2 for oracle/invocation causes, to BV0 for emitter
causes) is charter behaviour (BV0A.md:178-181) and is not re-specified here.

### 4.3 Validation order — the total order

Let the **contributing maps** be those fragments that are present and carry a non-empty `sourceMap`,
in the fixed order **script, then template**.

**Stage 0 — request and inventory.**

- **0.1** If `sourceMapRequested` is `false`: composition is not performed; the result carries code
  and no map (§7.7). No further check runs.
- **0.2** If the script's map is REQUIRED (§3.4) and absent → `MissingRequiredInputMap{script}`.
- **0.3** If the template's map is REQUIRED and absent → `MissingRequiredInputMap{template}`.

**Stage 1 — per contributing map.** The steps below run to completion for the **script** map first,
then for the **template** map. A fragment that is not a contributing map is skipped entirely.

- **1.1** The `sourceMap` bytes are an admissible JSON document. Three ordered clauses, first failure
  wins; together they are the **interoperable JSON domain** of §4.5, and they are checked before any
  member of the document is read:
  - **(a)** the bytes are well-formed JSON per RFC 8259 → else `U1.1`
  - **(b)** every JSON **number** in the document lies in the interoperable numeric domain: it denotes
    a finite IEEE-754 double, scanning the document in source order → else `U1.9`
  - **(c)** every JSON **string** in the document, after unescaping, is a sequence of well-formed
    Unicode scalar values — no unpaired surrogate, whether written literally or as a `\uD800`-style
    escape — scanning in source order → else `U1.10`
- **1.2** No object anywhere in the parsed document declares the same member name twice. → `U1.8`
- **1.3** The root is a JSON object. → `U1.2`
- **1.4** `version` is present. → `U2.1`
- **1.5** `version` is an integral JSON number. → `U2.2`
- **1.6** `version` equals `3`. → `U2.3`
- **1.7** No `sections` member. → `U5.1`
- **1.8** `mappings` is present. → `U1.3`
- **1.9** `mappings` is a JSON string. → `U1.4`
- **1.10** `sources` is present and an array. → `U1.5`
- **1.11** `names` is present and an array. → `U1.6`
- **1.12** `sourcesContent`, if present, is an array. → `U1.7`
- **1.13** `sourceRoot`, if present, is a string or `null`. → `U1.7`
- **1.14** `file`, if present, is a string or `null`. → `U1.7`
- **1.15** `ignoreList` / `x_google_ignoreList`, if present, is an array of non-negative integral
  numbers. If both spellings are present they must be deep-equal. → `U1.7`
- **1.16** `debugId`, if present, is a string. → `U1.7`
- **1.17** Every `sources` element is a string, scanned in **ascending index order**. → `U4.1`
- **1.18** Every `names` element is a string, scanned in **ascending index order**. → `U4.2`
- **1.19** Every `sourcesContent` element is a string or `null`, scanned in **ascending index
  order**. → `U4.3`
- **1.20** `sourcesContent`, if present, has the same length as `sources`. → `U4.4`
- **1.21** `mappings` decodes. The decode is a single left-to-right pass over the string, and segments
  are examined in wire order; the **first** violation is the outcome. **Within one segment the checks
  run in three ordered phases, and this ordering is mandated, not incidental** — real implementations
  differ here, because a decode-then-validate design and an apply-as-you-decode design naturally reach
  these checks in different orders:

  - **Phase A — lexical and per-field, as each field is read, in wire order** (generated column,
    source index, original line, original column, name index). A field's own encoding is well defined
    independently of how many fields the segment turns out to have:
    - a character outside the base64 alphabet → `U3.1`
    - the segment ends while a continuation bit is set → `U3.2`
    - the field's encoding continues past bit 31, or its value falls outside `[−(2^31−1), 2^31−1]`
      → `U3.4`
  - **Phase B — arity, once the segment has been read in full.** A field count other than 1, 4 or 5
    → `U3.3`.
  - **Phase C — accumulator application, and only if phase B passed.** The fields are applied to the
    running accumulators in wire order; for each:
    - the accumulator (`genCol`, `srcIdx`, `srcLine`, `srcCol`, `nameIdx`) becomes negative or exceeds
      `2^31−1` → `U3.5`
    - then, for `genCol` only, it is strictly less than the previous segment's on the same generated
      line → `U3.6`

  **Arity therefore beats every accumulator property.** A segment decoding to three fields whose
  first field would also drive an accumulator negative reports `U3.3`, not `U3.5`: a 3-field segment
  has no defined interpretation at all, so naming its field 0 a "generated column delta" and its
  effect an "underflow" already presumes an interpretation the wire format does not give it.
  Accumulators are not touched until the segment's arity is known legal. Within phase C, range beats
  ordering: a `genCol` driven negative is `U3.5`, while one that decreases but stays in range is
  `U3.6`.
- **1.22** Every segment's table indices are in bounds. Segments are scanned in **wire order**;
  within one segment, `srcIdx` is checked before `nameIdx`. Both checks are guarded on the field being
  **non-null**, because a 1-field segment is sourceless by definition (§2.2) — all four authored fields
  are `null` — and `null` is in no index range; an unguarded check would reject every sourceless
  segment and take the whole sourceless-barrier algebra (§5.4) with it:
  - a **non-null** `srcIdx` not in `[0, sources.length)` → `U6.1`
  - a **non-null** `nameIdx` not in `[0, names.length)` → `U6.2`
- **1.23** Every ignore-list entry is in `[0, sources.length)`, scanned in **ascending index order**.
  → `U6.3`
- **1.24** Every segment's **generated** coordinate is usable. Segments are scanned in **wire
  order**; within one segment the three checks are in this order:
  - `genLine` not in `[0, lineTable(code).length)` → `U7.1`
  - `genCol` not in `[0, lineTable(code)[genLine].length]` → `U7.2`
  - `genCol ≥ 1` and the UTF-16 code unit at index `genCol−1` of the line is a high surrogate and the
    unit at index `genCol` is a low surrogate → `U7.3`

**Stage 2 — across the contributing maps.** Stage 2 runs over the **set of contributing maps
whatever its cardinality**, including a set of exactly one; it is not conditional on both fragments
carrying maps.

- **2.1** All contributing maps agree on the normalised `sourceRoot` (§7.5). With a single
  contributing map this is vacuously satisfied and that map's `sourceRoot` carries through
  unchanged. → `U8.1`

**`DECISION` D-8 — `U8.1`'s `fragment` attribution.** §4.2 requires `UncomposableInputMap{family,
code, fragment}` to name a fragment, but a stage-2 check reads every contributing map at once, and
prose alone left which one to name unspecified. Two independent implementations, each written
against revision 7 with no visibility into the other, resolved this identically without a shared
rule to point to — the frozen text left a real gap, not merely an unwritten convenience.

> `U8.1`'s `fragment` is the **template**. `U8.1` can fire only when BOTH fragments are contributing
> maps: with zero or one contributing map, 2.1's agreement condition is vacuously satisfied (§4.3's
> own text), and the DTO (§3.3) has exactly two map-carrying slots, so "both contribute" is the only
> reachable firing condition. Contributing-map order is fixed as script, then template (§4.3). So
> every reachable `U8.1` failure is a two-map disagreement in which the template's normalised
> `sourceRoot` is being read and compared against the script's — already read first — and named as
> the map that did not match.

**This rule is scoped to the present two-fragment DTO (§3.3) and is not claimed to generalise.** A
future DTO admitting a third mapped fragment would need its own re-derivation — §11.6 item 4
already places multi-fragment futures out of scope here, and "the later contributing map in a fixed
order" stops being unambiguous the moment more than one map could disagree with an established
baseline (a first-mismatch scan and a last-in-list reading diverge as soon as three-plus
contributors are possible). D-8 decides only the two-fragment case, unconditionally: `template`.
**Rejected alternative:** name the script, on the reasoning that the check is symmetric and either
fragment is an equally valid witness. Rejected because a symmetric check still has an asymmetric
*evaluation* order under this specification's own fail-fast, ordered-stages discipline (§4.1, §4.3's
stage-1 script-then-template order), and naming the fragment whose value is *compared against* the
already-read one — rather than an arbitrary pick between two structurally identical maps — is the
one reading consistent with how every other stage-1 check in this document already attributes a
failure to the map being read, not to some other map already validated clean.

Stage-order tie-breaks a reviewer should confirm are deliberate:

- **Duplicate-member detection precedes every member read** (1.2), so no later check can silently
  read whichever duplicate the parser happened to keep.
- **Version beats indexed-map** (1.6 before 1.7): a `version: 2` map that also carries `sections`
  reports `U2.3`.
- **Indexed-map beats missing `mappings`** (1.7 before 1.8): an indexed map legitimately has no
  `mappings`.
- **Row typing beats wire decoding** (1.17–1.20 before 1.21): index-bounds and coordinate checks
  presuppose a typed table.
- **`sources` rows beat `names` rows beat `sourcesContent` rows** (1.17, 1.18, 1.19): two malformed
  elements in different arrays have a determined winner.
- **Arity beats index bounds** (`U3.3` before `U6.1`): `"AC"` decodes to two fields and is `U3.3`,
  never `U6.1` — the distinction seed vector F5 depends on.
- **Index bounds beat coordinate bounds** (1.22 before 1.24): a segment that is both dangling-index
  and out-of-fragment reports `U6`. Note this is a *stage* precedence, not a per-segment one: an
  index violation in segment 9 beats a coordinate violation in segment 2.
- **Script beats template** (stage 1 order): a malformed script map and a dangling-index template map
  report the script's outcome.

### 4.4 The exhaustive taxonomy

The eight families are the charter's ratified list (BV0A.md:255-259), refined into exact sub-codes as
AMD-008:344-350 requires. Every sub-code belongs to exactly one family; an input passing every
sub-code is composable.

**U1 — malformed map JSON.** *Read as: the document is not a well-formed source-map v3 object.*

| Code | Precondition |
|---|---|
| `U1.1 map-bytes-not-json` | The `sourceMap` string is not parseable JSON. |
| `U1.2 map-root-not-object` | The parsed root is not an object. |
| `U1.3 mappings-member-absent` | No `mappings` member. **An absent `mappings` is never read as an empty map.** |
| `U1.4 mappings-member-not-a-string` | `mappings` present but not a string. |
| `U1.5 sources-member-absent-or-not-an-array` | `sources` absent, or present and not an array. |
| `U1.6 names-member-absent-or-not-an-array` | `names` absent, or present and not an array. |
| `U1.7 metadata-member-wrong-type` | `sourcesContent` not an array; `sourceRoot` neither string nor `null`; `file` neither string nor `null`; an ignore list that is not an array of non-negative integers, or two disagreeing ignore-list spellings; `debugId` not a string. |
| `U1.8 duplicate-object-member` | Any JSON object in the document declares the same member name twice. |
| `U1.9 number-outside-interoperable-domain` | A JSON number in the document does not denote a finite IEEE-754 double — a magnitude beyond the finite double range being the motivating case (`1e400`). |
| `U1.10 string-not-well-formed-unicode` | A JSON string in the document contains an unpaired surrogate after unescaping (`"\uD800"` being the motivating case). |

**U2 — wrong/missing version.**

| Code | Precondition |
|---|---|
| `U2.1 version-member-absent` | No `version` member. |
| `U2.2 version-not-an-integer` | `version` present but not an integral JSON number. |
| `U2.3 version-not-3` | `version` is an integer other than `3`. |

**U3 — undecodable or out-of-range wire data.**

| Code | Precondition |
|---|---|
| `U3.1 vlq-invalid-character` | A `mappings` character outside the base64 alphabet appears inside a segment. |
| `U3.2 vlq-truncated-segment` | A segment ends while a continuation bit is set. |
| `U3.3 segment-field-count` | A decoded segment has a field count other than 1, 4 or 5. |
| `U3.4 vlq-field-out-of-range` | A single field's encoding continues past bit 31, or its value falls outside `[−(2^31−1), 2^31−1]`. The accepted decoder is lenient here — `"A"` and `"ggggggE"` both yield 0 because a 32-bit shift wraps — and only the conforming encoding is admissible. |
| `U3.5 accumulator-out-of-range` | A running accumulator becomes negative or exceeds `2^31−1`. |
| `U3.6 generated-column-accumulator-decreased` | Within one generated line, a segment's `genCol` is strictly less than the previous segment's. |

**U4 — malformed table rows.**

| Code | Precondition |
|---|---|
| `U4.1 source-row-not-a-string` | A `sources` element is not a string. |
| `U4.2 name-row-not-a-string` | A `names` element is not a string. |
| `U4.3 sources-content-row-not-string-or-null` | A `sourcesContent` element is neither a string nor `null`. |
| `U4.4 sources-content-length-mismatch` | `sourcesContent` is present and its length differs from `sources`. |

**U5 — indexed/non-flat map.**

| Code | Precondition |
|---|---|
| `U5.1 sections-member-present` | A `sections` member is present, with any value. A conforming consumer prefers `sections` over `mappings`, so such a map does not describe its generated document through `mappings`. |

**U6 — dangling table index.**

| Code | Precondition |
|---|---|
| `U6.1 source-index-out-of-table` | A segment's **non-null** `srcIdx` is not in `[0, sources.length)`. A sourceless segment carries `srcIdx = null` (§2.2) and is never an instance of this code. |
| `U6.2 name-index-out-of-table` | A segment's non-null `nameIdx` is not in `[0, names.length)`. |
| `U6.3 ignore-list-index-out-of-table` | An ignore-list entry is not in `[0, sources.length)`. |

**U7 — out-of-fragment or surrogate-split coordinate.** Checked against the fragment's own `code`, in
its own pre-rewrite coordinate space.

| Code | Precondition |
|---|---|
| `U7.1 generated-line-out-of-fragment` | `genLine` not in `[0, lineTable(code).length)`. |
| `U7.2 generated-column-out-of-fragment` | `genCol` not in `[0, lineTable(code)[genLine].length]`. |
| `U7.3 generated-column-splits-a-surrogate-pair` | `genCol ≥ 1`, the UTF-16 unit at index `genCol−1` is a high surrogate, and the unit at index `genCol` is a low surrogate — so `genCol` addresses no character boundary and no byte offset exists for it. (The `genCol ≥ 1` guard makes the predicate total: column 0 can never split a pair.) |

**U8 — incompatible cross-fragment table metadata.**

| Code | Precondition |
|---|---|
| `U8.1 source-root-conflict` | Two contributing maps declare different normalised `sourceRoot` values (§7.5). A composed map has one `sourceRoot` for all rows, so it cannot represent both without perturbing a declared source identity. |

**Original coordinates are NOT validated.** `srcLine` / `srcCol` are authored-side coordinates,
carried opaquely (BV0A.md:19-22). BV0A holds no authored file to validate them against, and the
oracle's own `original-position-bounds` check (`src/mapping-oracle.mjs:1200-1207`) is BF2's, not
BV0A's. The only constraint on them is `U3.5`'s accumulator range. A mechanically composable but
oracle-invalid mapping is carried forward faithfully and is a BV0 bug, not a BV0A rejection
(BV0A.md:267-271).

**`DECISION` D-1 — a decreasing same-line generated column is rejected (`U3.6`).** A flat map may
encode a negative generated-column delta; `mappings: "K,F"` decodes to a sourceless segment at
`(0,5)` followed by one at `(0,3)`, and every other check in §4.3 accepts it. Layer 1 rejects it.
Rationale: the composition is a **positional** walk (§5.3) that visits emission points in increasing
offset order, so it would emit those two segments in the opposite order from the one declared —
reordering a declared segment order, which BV0A.md:19-22 forbids outright. Accepting-and-preserving
is not available: preserving declared order and walking positionally are mutually exclusive, and the
accepted lookup does not observe the declared order for decreasing columns anyway, because
`resolveAt` sorts each line by `genCol` first (`src/mapping-oracle.mjs:1171`). Rejecting therefore
protects the one property the charter names, and makes §5.5's and §7.6's non-decreasing claims true
by construction rather than by assumption. It is filed under U3 as a sibling of the `U3.5`
accumulator rule: both state that a running wire accumulator moved somewhere the composition cannot
follow. Rejected alternative: accept, preserve declared order, and encode with signed column deltas —
rejected because the positional walk cannot preserve that order, so "preserve" would in fact mean
"reorder into positional order", which is the forbidden operation wearing a different name.

**`DECISION` D-2 — a duplicate JSON object member is rejected (`U1.8`).** RFC 8259 leaves the meaning
of a repeated member name undefined, and the two languages' default object models happen to agree on
last-wins only by coincidence, not by contract. Layer 1 will not let an undefined-behaviour case be
resolved by whichever parser an implementation happens to use. This imposes a real obligation:
`JSON.parse` and `serde_json::Value` both collapse duplicates silently, so **an implementation must
detect duplicates before or during parsing** — a top-level-and-nested duplicate-name scan — and an
implementation that relies on its parser's object model alone does not satisfy `U1.8`. Rejected
alternative: specify last-wins. It is deterministic and free, and it is a defensible choice; it is
rejected because it silently assigns meaning to a document that has none, which is the same class of
move as reading an absent `mappings` as empty (`U1.3`), and because the detection cost is small and
one-time. Unreachable for real producers: the in-tree serializer emits each member once
(`oxc_sourcemap-7.0.0/src/encode.rs:119-165`).

### 4.5 The interoperable JSON domain

"Parses as JSON" is not one thing across languages, and this specification is checked by one
implementation in JavaScript against another in Rust (AMD-008:220-224). Two documents make the
difference observable:

- `{"version": 1e400, …}` — a number whose magnitude exceeds the finite IEEE-754 double range. One
  language's parser may reject the document outright; another may accept it and coerce the value to an
  infinity.
- `{"sources": ["\uD800"], …}` — a string containing an unpaired surrogate. One language's parser may
  accept it into a UTF-16-based string type; another may reject it when materialising a
  well-formed-Unicode string type.

Left to each parser, the same bytes then produce two different outcomes — a rejection here, a coerced
value there — and the two implementations disagree without either being wrong about *its own* language.
Layer 1 therefore fixes the domain rather than inheriting it:

> **Only documents in the interoperable JSON domain are admissible input.** A JSON number must denote a
> finite IEEE-754 double; a JSON string must, after unescaping, be a sequence of well-formed Unicode
> scalar values. Everything else is rejected at §4.3 step 1.1 by `U1.9` or `U1.10`, **regardless of
> whether the implementation's native parser would have accepted it**, and before any member of the
> document is read.
>
> **The conversion is IEEE-754 binary64 using round-ties-to-even**, applied to the number's exact
> decimal lexeme; a number is in-domain iff that conversion is finite. **Every later numeric predicate
> — integrality (`U2.2`), equality (`U2.3`), non-negativity and integrality of ignore-list entries
> (`U1.7`), and index comparison (`U6.3`) — operates on the CONVERTED binary64 value, never on the
> exact decimal lexeme.**

This carries the same kind of obligation `U1.8` does, in both directions: an implementation whose
parser is more permissive must detect the out-of-domain value itself rather than proceed with a coerced
one, and an implementation whose parser is stricter must report `U1.9` / `U1.10` rather than let a
parse-time rejection surface as `U1.1`. Neither may delegate the decision to its parser. Placing both
checks inside step 1.1, ordered after JSON syntax and before every member read, makes the outcome
independent of which of the two an implementation happens to notice first.

The domain is a **precondition on input maps**, parallel to precondition P1 on DTO strings (§3.5). It
is unreachable for real producers, whose maps carry only the small integers of a decoded map plus
source paths and contents drawn from real files.

**`DECISION` D-7 — numbers are binary64 values, not exact decimals.** Fixing the domain leaves one
question the source-map format does not answer: once a lexeme is admitted, is a later predicate asked
about the decimal the document wrote or about the double it converts to? The two differ. Under an
exact-decimal reading `{"version": 3.0000000000000000001}` is non-integral and rejected by `U2.2`;
under the binary64 reading it converts to exactly `3` and is accepted. Specifying the rounding mode is
what makes the domain boundary itself exact as well: with round-ties-to-even, a magnitude slightly
above the largest finite double still converts to that finite double rather than to an infinity, so
"in-domain iff the conversion is finite" names a determinate threshold instead of an approximate one.

**Binary64 is chosen.** Rationale: it is what a JSON parser in either language produces without being
asked, so the rule makes explicit the reading both implementations would otherwise arrive at silently —
which is the point of fixing it, since *silently agreeing* is exactly the common-mode condition
AMD-008's two-implementation design exists to break. Rejected alternative: exact-decimal semantics.
That would oblige both implementations to carry an arbitrary-precision decimal parser solely to reject
documents no producer emits, and it would put the two on different footings, since neither language's
standard JSON parser preserves the lexeme. Unreachable for real producers either way: the only numbers
in a real input map are a version of `3` and, in principle, small non-negative ignore-list indices, all
exactly representable.

---

## 5. The rewrite/chaining algebra

### 5.1 Overview

For a fragment `F` with code `C` and validated map `M` (a table plus a segment sequence in `C`'s
coordinate space):

1. **Pass 1** (script only): build a `CodeTransform` over `C`, apply the global `__sfc__` →
   `_sfc_main` overwrite, obtain `C₁ = build_string()` and the chunk list `K₁`. Chain `M` through
   `K₁` per §5.3 to obtain `M₁`, a segment sequence in `C₁`'s coordinate space carrying `M`'s
   payloads.
2. **Pass 2** (script only): build a `CodeTransform` over `C₁`, apply the global
   `export default _sfc_main;\n` removal, obtain `C₂` and `K₂`. Chain `M₁` through `K₂` to obtain
   `M₂`.
3. **Placement**: translate `M₂` (script) or `M` (template) by the fragment's placement (§6.3).
4. **Assembly**: concatenate the placed sequences in write order, add the boundary segments §6.4
   requires, and compose the tables per §7.4.

Passes are applied **sequentially**, pass 2 over pass 1's output coordinate space, exactly as the
code does (§2.5). A pass whose pattern has zero occurrences is still a pass: its chunk list is
`[Original{0,len}]` (CT-1) and §5.3 still applies — with no `Overwritten` chunk it is the identity on
`M`. Both passes are modelled as declaring a source — the pass's own input text — which is the
configuration CT-5 and CT-8's guards require.

**Empty fragment code.** For an empty `C` the chunk list is empty (CT-1), so rules (a), (b) and (c)
of §5.3 have nothing to visit. Rule **(d) still fires**: it is not a per-chunk rule, and `len(C) = 0`
is a position `M` may legitimately declare — `lineTable("")` is `[""]`, so `(0,0)` is the one
in-bounds position and `U7.2` admits it. A segment `M` declares at `(0,0)` over empty code is
therefore carried, at the fragment's placement position. Dropping it would be a drop
(BV0A.md:19-22). This is the only case in which §5.3 emits from a fragment with no chunks.

### 5.2 Position conversion, and the boundary lemma

Within one fragment's coordinate space:

- `pos(o)` — the `(line, column)` of byte offset `o`: `line` is the number of LF bytes strictly before
  `o`; `column` is the UTF-16 length of the bytes from that line's start up to `o`. This is
  `offset_to_line_and_col` reduced to 0-based (CT-12).
- `off(line, column)` — the byte offset of an in-bounds position: the byte offset of `line`'s start
  plus the byte length of the first `column` UTF-16 code units of `lineTable(t)[line]`.

**Boundary lemma.** `pos` is only ever applied to offsets that are UTF-8 character boundaries, so it
is always well defined:

- chunk starts and ends are `0`, `len`, or an edit boundary (CT-4), and both rewrite patterns are
  pure ASCII (§2.5), so every match boundary is a character boundary;
- input-segment offsets are produced by `off(line, column)`, which by construction lands on a
  character boundary — `U7.3` has already rejected the one wire column that does not correspond to
  one.

`pos` and `off` are mutual inverses on that domain. Every conversion happens at this one boundary;
byte offsets and UTF-16 columns are never mixed (BV0A.md:90-92).

### 5.3 The chain operation

Let `K` be a pass's chunk list over text `T`, and `M` the segment sequence to be chained (in `T`'s
coordinate space). Define:

- `seg(o)` = the sub-sequence of `M` whose position equals `pos(o)`, in `M`'s order;
- `lookup(o)` = `resolveAt(M, pos(o))` (§2.3) — *absent*, or a segment.

Walk `K` in chunk order, maintaining the output generated position (advancing exactly as
`advance_generated_position` does — LF increments the line and resets the column, otherwise the
column advances by UTF-16 length; `source_map.rs:577-591`). Emit, in walk order:

**(a) `Original{s,e}`.** Its **emission points** are

> `{ s | s is the end of a replaced range of this pass } ∪ { o ∈ [s,e) : seg(o) is non-empty }`

taken in increasing offset order. The first clause's condition is equivalent to "this chunk is
immediately preceded in chunk order by an `Overwritten` chunk", because chunk boundaries are exactly
`{0} ∪ {edit starts} ∪ {edit ends} ∪ {len}` (CT-4) and a replaced range is always exactly one chunk;
it is false for the pass's first chunk unless that chunk itself follows a replacement beginning at
offset 0. At each emission point `o`, at the output generated position
corresponding to `o`:

- if `seg(o)` is non-empty → emit **every** segment of `seg(o)`, in `M`'s order, each carrying its own
  payload unchanged;
- otherwise → emit **one** segment carrying `lookup(o)`'s payload, or a **sourceless** segment if
  `lookup(o)` is absent.

**(b) `Overwritten{s,e,content}` with non-empty `content`.** Emit exactly **one** segment at the
replacement's generated start, carrying `lookup(s)`'s payload, or sourceless if `lookup(s)` is absent.
The generated position advances by `content`.

**(c) `Overwritten{s,e,""}`.** Emit **nothing**; the generated position advances by zero.

**(d) End of walk.** If `M` contains segments at offset `len(T)` — the position one past the last
byte — emit them, in `M`'s order, at the output's end position. They are covered by no chunk and would
otherwise be silently dropped. **That position is always in-bounds and this rule is always live**: when
`T` ends with LF it is `(lastLine, 0)` on the trailing empty line; when `T` does not end with LF it is
end-of-line on the last line, which §2.1 and `U7.2` both admit; and when `T` is empty it is `(0,0)`,
the case §5.1 covers. It is not conditioned on `T`'s final byte, and an implementation that guards it
on a trailing LF drops legitimate segments.

**Rule precedence, stated so the walk is total.** Rules (b) and (c) govern the **whole** replaced
range `[s,e)`: every segment of `M` whose offset lies in `[s,e)` is **dropped**, whatever its
multiplicity, and the emission-point machinery of rule (a) applies **only inside `Original` chunks**.
There is no case in which both (a) and (b)/(c) speak about one offset: the chunk list partitions
`[0,len(T))` (CT-4), and offset `len(T)` is handled by (d) alone.

The emitted sequence is `M`'s chained image, non-decreasing in generated position by construction.

**Totality of the walk.** A pass's chunk list partitions `[0, len(T))`: the initial chunk is
`Original{0,len}` (CT-1) and every edit replaces a covering run with that run's own partition (CT-4).
Every segment of `M` therefore falls in exactly one chunk, or at offset `len(T)` and is handled by
(d). No segment can be silently lost.

**Terminal vs non-terminal removal.** Rule (c) emits no token of its own, so what follows the removed
range decides:

- a **non-terminal** removal is followed by a surviving `Original` chunk whose start is the removed
  range's end, so rule (a)'s first emission point fires there, carrying `lookup(that offset)` — which
  is sourceless whenever that line declares no applicable segment at or before it;
- a **terminal** removal — one whose range ends at `len(T)` — has no following chunk and therefore
  produces **no** transition segment at all. The only thing that can appear at that generated
  position is a rule-(d) segment.

The same asymmetry applies to a non-empty overwrite: its own segment comes from (b), and the segment
at the *old end* comes from the following chunk under (a), if there is one.

**The relationship to `CodeTransform`'s own tokens, stated precisely.** Emitted segments come from two
structurally different sources, and conflating them overstates the relationship:

- **Pass tokens.** Rule (b) is CT-8 verbatim; rule (c) is CT-9 verbatim; the first clause of rule (a)
  is the own-start token CT-5 emits for the `Original` chunk that resumes surviving text after a
  replacement, at CT-5's position. Of these, this specification carries all three and **omits** two
  further kinds `CodeTransform` emits: the own-start token of a first chunk that does not resume from a
  replacement, and the interior line-start tokens of CT-6. So the set of pass tokens carried is a
  strict **subset** of the pass map's tokens, at their own positions and with their own geometry.
- **Carried input segments.** Rule (a)'s second clause and rule (d) emit at positions `M` declares,
  translated through the pass's **chunk structure** — which chunk contains the offset, and where that
  chunk lands in the output. These are **not** `CodeTransform` tokens and are not claimed to be: with
  the empty `sourcemap_locations` set both passes have (CT-13), `generate_map` emits nothing at an
  interior input-segment position, and the coincident-segment case (§5.5 rule 1) has no token
  representation at all, since `sourcemap_locations` is deduplicated (`source_map.rs:131-133`).

The accurate claim is therefore **not** "the output is a subset of the pass map". It is:

> Every geometric decision this specification makes is `CodeTransform`'s own chunk list; it emits no
> pass token `CodeTransform` would not emit, at no position `CodeTransform` would not emit it; and the
> positions at which it carries input segments are those segments' own declared positions composed
> with that same chunk structure.

That is what forecloses AMD-008 §1 item 3's hazard — two models of one operation that *disagree* about
geometry. `DECISION` D-3 (§11.2) states why the two pass-token kinds are omitted, and proves under
§2.6's standard that omitting them changes `payloadAt` nowhere.

**Payload precedence at an emission point.** When `seg(o)` is non-empty, `lookup(o)` is by definition
the LAST segment of `seg(o)` — later segments on the line are at greater columns — so the suppressed
resume segment would have been a byte-identical duplicate of the last emitted segment. Suppression
removes a duplicate; it never removes information. This is §2.6's standard applied at the emission
point: a segment that would duplicate one already emitted at the same coordinate changes `payloadAt`
nowhere, for any input, so it is not emitted — the same test that excludes the two pass-token kinds
above.

**Names.** A chained segment carries the looked-up segment's `nameIdx` unchanged. A rewrite never
drops, invents, or renames a name binding; whether a carried name is *truthful* is BF2/BV0's concern.

### 5.4 Sourceless-barrier semantics

The barrier is not a special rule; it is a consequence of `lookup` being **last applicable segment,
whatever it is** rather than **last source-bearing segment**:

> If the last applicable segment at `pos(o)` is sourceless, the chained segment at `o` is sourceless.
> If there is no applicable segment on `pos(o)`'s line at or before its column — even though a
> source-bearing segment exists on an earlier line, or further right on the same line — the chained
> segment is sourceless.

An implementation that treats a sourceless segment as transparent, or that falls through to a previous
line, fabricates authored provenance for a region the fragment deliberately left unmapped. Both are
forbidden. A chained emission point whose lookup is absent emits a **sourceless segment**, not
nothing: dropping it would let a consumer inherit the segment to its left on the same line.

### 5.5 Equal-coordinate ordering and collision policy — total statement

Two segments may share a generated coordinate. The order of the assembled sequence is fully determined
by these rules, in this precedence:

1. **Inside an `Original` chunk**, emission points are visited in increasing offset order, and at one
   emission point coincident input segments are emitted in `M`'s order, with the resume segment
   suppressed when any coincident input segment exists (§5.3(a)).
2. **Inside a replaced range**, rules (b)/(c) alone apply: whatever the number of input segments at or
   inside `[s,e)` — zero, one, or N coincident at `s` — a non-empty overwrite emits **exactly one**
   segment and an empty one emits **none**. Rule 1 does not reach into a replaced range.
3. **Two emitted segments of one pass that are not input segments can never share a coordinate.**
   Proof: every chunk that emits is non-empty and advances the generated position (CT-4 produces no
   empty `Original`; an empty `Overwritten` emits nothing per CT-9); within one `Original` chunk each
   emission point is a distinct offset separated by at least one byte of advance; and a resume segment
   exists only at a chunk start. A future edit primitive able to produce a zero-length emitting chunk
   would break this proof and would require an amendment.
4. **Across the rename → removal chain**, pass 2's input is pass 1's chained output and rules 1–3
   apply recursively. Chaining preserves relative order: the walk visits generated positions
   monotonically, and coincident segments of `M₁` are all found at one emission point of pass 2 and
   emitted in `M₁`'s order.
5. **Across fragments**, the assembled sequence is the **concatenation in assembly write order** —
   every placed script segment precedes every placed template segment. Under the real write grammar
   their line ranges are disjoint (§6.5); the rule is stated unconditionally so that it stays total if
   that were ever not the case.
6. **A fragment-end boundary segment (§6.4) is emitted after every placed segment of the fragment it
   bounds**, and therefore wins the `resolveAt` tie at its own coordinate. There is no
   fragment-*start* boundary segment; §6.4 proves none is needed.

`U3.6` guarantees every input sequence is non-decreasing, and rules 1–6 preserve that, so the
assembled sequence is non-decreasing in `(genLine, genCol)`.

**Worked case — a replaced range beginning at column 0 with N coincident prior segments.** Let the
script code begin with `__sfc__` at offset 0 and let `M` declare three segments at `(0,0)`. Pass 1's
chunk list is `Overwritten[0,7) | Original[7,…)`. Rule 2 governs `[0,7)`: all three input segments
are inside it and are dropped, and exactly **one** segment is emitted at generated `(0,0)` carrying
`lookup(0)` — the payload of the **third** input segment, because `resolveAt` takes the last
applicable. Rule 1 then governs `Original[7,…)`, whose start is a replaced range's end, so a resume
segment is emitted at generated `(0,9)` carrying `lookup(7)`. The answer is unique: two segments,
`(0,0)` and `(0,9)`.

### 5.6 What the composition is not

It is not a naive outer-driven map chain. A chain that iterates only the pass map's tokens and looks
each up in `M` would drop every input segment not sitting at a chunk start — including one of two
coincident segments at the same coordinate, which seed vector V3 exists to pin. The no-drop
requirement (BV0A.md:19-22) forces the input-segment ingredient of rule (a).

### 5.7 The template fragment is not rewritten

Neither pass applies to the template fragment: `compile.rs:178-188` writes `template.code` verbatim and
the two `replace` calls at `compile.rs:84-85` operate on the script's clone only. The template's map is
therefore **placed directly** (§6.3), with no chain step. Modelling the template as passing through an
identity `CodeTransform` would be a no-op under §5.3 (no replaced ranges ⇒ no emission points beyond
the input segments themselves), but stating it as "no chain step" removes any doubt.

### 5.8 A present fragment whose map is legitimately absent

§3.4 permits a fragment to be present — contributing bytes to the module — while carrying no map: the
present-but-not-authored synthetic script of a template-only cell is the real instance, and
`sourceMapRequested == false` makes it universal. This is its own case, not a special case of the
general rule, and it is stated here rather than left to be inferred:

> A present fragment with an empty `sourceMap` string is **not a contributing map**. Its code is still
> written, and for the script still rewritten by both passes — the passes determine the module's
> BYTES, and the code baseline is pinned regardless of any map. But the fragment contributes
> **nothing** to the assembled map: no carried segments, no replacement segments, no resume segments,
> no table rows, no ignore-list entries, and **no BR-3 boundary segment**.

There is no `M` to chain, so §5.3 is not invoked for it at all; treating its map as a validated-but-empty
sequence would be a different rule with a different result, because §5.3(b) and the resume clause of
§5.3(a) emit sourceless segments when `lookup` is absent, and an empty `M` makes `lookup` absent
everywhere — a mapless fragment would then sprout a sourceless segment at every replacement and every
resume position. Those segments would fail the emission standard (§2.6): with no other segment anywhere
in the fragment's line range, every one of them sits where `resolveAt` is already absent, for every
admissible input. The boundary segment is excluded by the same standard — §6.4 BR-3 exists to stop a
lookup inheriting a *fragment* segment, and a mapless fragment has none to inherit.

The term **contributing map** is used throughout §4, §6.4 and §7 with exactly this meaning: a fragment
that is present and carries a non-empty `sourceMap` which passed §4's validation.

---

## 6. Assembler write-site manifest and boundary rules

### 6.1 Classification vocabulary

- **(A) mapped fragment content** — bytes that are a fragment's own code, carried verbatim (the
  template) or after the two authorized rewrites (the script).
- **(B) assembly-owned synthetic content** — bytes the assembler itself composes. They carry no
  mapping unless a genuine authored-source mapping exists (BV0A.md:87-90); none does.
- **(C) transition** — the boundary between (A) and (B), in either direction.

### 6.2 The manifest, as an exact byte grammar

Two formatting functions, matching the `write!` verbs the source uses:

- `raw(s)` — the bytes of `s`, verbatim, with **no escaping of any kind**. This is Rust's `{}` for
  `&str`. A `"` or `\` in such a string therefore reaches the generated JavaScript unescaped; that is
  existing behaviour, the code baseline is byte-pinned, and repairing it is outside BV0A's scope.
- `dbg(s)` — Rust's `{:?}` for `&str`: a `"`, then each character mapped by `"` → `\"`, `\` → `\\`,
  LF → `\n`, CR → `\r`, TAB → `\t`, and every other character verbatim, then a closing `"`. **Under
  precondition P1 (§3.5) this is the complete definition**; outside P1 Rust additionally applies
  `\u{…}` escaping driven by its Unicode printability and grapheme-extended tables, which this
  specification does not reproduce and does not define behaviour for.

Two derived id renderings, from `render_ids` (`id.rs:183-228`):

- `styleId(i)` = `raw(canonicalId) ++ "?vue&type=style&index=" ++ dec(i) ++ "&lang." ++ raw(L)` where
  `L` is `styleLangs[i]` when that index exists and is non-null, else `"css"` (`id.rs:201-215`).
- `customId(i)` = `raw(canonicalId) ++ "?vue&type=" ++ raw(B) ++ "&index=" ++ dec(i)` where `B` is
  `customTypes[i]` when that index exists, else `"custom"` (`id.rs:216-226`).

`dec(i)` is the decimal rendering of a `usize` with no separators or sign.

Every write performed by `assemble_vue_main_module` (`compile.rs:70-260`), in execution order. Script
output now precedes the template's own runtime imports (the assembler writes the authored script's own
imports and body first, then the template's runtime-helper imports immediately before the render
function), matching official `@vitejs/plugin-vue`/`@vue/compiler-sfc` order — proven against the rc.3
goldens; W-04/W-05 accordingly sit after W-06/W-07 (or W-08/W-09) below, not before them:

| Site | Lines | Condition | Exact bytes | Class |
|---|---|---|---|---|
| W-01 | `:102-105` | for each `i` in `0..styleCount` | `import "` ++ `styleId(i)` ++ `"\n` | B |
| W-02 | `:107-110` | for each `i` in `0..customBlockCount` | `import block` ++ `dec(i)` ++ ` from "` ++ `customId(i)` ++ `"\n` | B |
| W-03 | `:112-114` | `styleCount > 0 ∨ customBlockCount > 0` | `\n` | B |
| **W-06** | `:126-143` | script present | **the rewritten script code, byte for byte** | **A** |
| W-07 | `:144-146` | script present ∧ rewritten code does not end LF | `\n` | B |
| W-08 | `:147-148` | script absent | `const _sfc_main = {}\n` | B |
| W-09 | `:149-151` | script absent ∧ `scopeId` non-empty | `_sfc_main.__scopeId = "` ++ `raw(scopeId)` ++ `"\n` | B |
| W-04 | `:154-165` | template present ∧ `imports` non-empty | `import { ` ++ join(`", "`, map(spec, imports)) ++ ` } from "` ++ `raw(R)` ++ `"\n` | B |
| W-05 | `:167-176` | template present ∧ `ssrImports` non-empty | `import { ` ++ join(`", "`, map(spec, ssrImports)) ++ ` } from "vue/server-renderer"\n` | B |
| W-10 | `:177` | template present | `\n` | B |
| **W-11** | `:178-188` | template present | **the template code, byte for byte** | **A** |
| W-12 | `:189-191` | template present ∧ code does not end LF | `\n` | B |
| W-13 | `:192-196` | template present ∧ its code contains `function ssrRender(` | `_sfc_main.ssrRender = ssrRender\n` | B |
| W-13′ | `:194-196` | template present ∧ not W-13 ∧ its code contains `function render(` | `_sfc_main.render = render\n` | B |
| W-14 | `:199-205` | for each `i` in `0..customBlockCount` | `if (typeof block` ++ `dec(i)` ++ ` === 'function') block` ++ `dec(i)` ++ `(_sfc_main)\n` | B |
| W-15 | `:207-209` | `¬isProduction` | `_sfc_main.__file = ` ++ `dbg(canonicalId)` ++ `\n` | B |
| W-16 | `:211-223` | `¬isProduction ∧ ¬ssr ∧ hmrStrategy = vite` | `/* HMR(vite) */\nif (import.meta.hot) { import.meta.hot.accept(() => {}) }\n` | B |
| W-16′ | `:211-223` | `¬isProduction ∧ ¬ssr ∧ hmrStrategy = webpack` | `/* HMR(webpack) */\nif (module.hot) { module.hot.accept(() => {}) }\n` | B |
| W-17 | `:232-250` | `ssr ∧ emitSsrModuleRegistration` | `import { useSSRContext as __vite_useSSRContext } from "` ++ `raw(R)` ++ `"\n` ++ `const _sfc_setup = _sfc_main.setup\n` ++ `_sfc_main.setup = (props, ctx) => {\n` ++ `  const ssrContext = __vite_useSSRContext()\n` ++ `  ;(ssrContext.modules \|\| (ssrContext.modules = new Set())).add(` ++ `dbg(S)` ++ `)\n` ++ `  return _sfc_setup ? _sfc_setup(props, ctx) : undefined\n` ++ `}\n` | B |
| W-18 | `:252` | always | `export default _sfc_main` (no trailing LF) | B |

where `R` = `runtimeModuleName` if non-null else `"vue"` (`compile.rs:156`, `:233`); `S` =
`ssrModuleId` if non-null else `canonicalId` (`compile.rs:242`); and `spec(n)` is
`format_import_specifier` (`crates/verter_compiler/src/compile/helpers.rs:148-158`): for a name
beginning with `_` and longer than one character, `raw(n[1..]) ++ " as " ++ raw(n)`; otherwise
`raw(n)`.

The four **transitions** are **T1** into the script (immediately before W-06), **T2** out of the
script (immediately after W-06's last byte, i.e. **before** W-07), **T3** into the template
(immediately before W-11), and **T4** out of the template (immediately after W-11's last byte, i.e.
**before** W-12).

Notes:

- W-13's choice is made by a **text scan of the template code** (`compile.rs:192`, `:194`); it is a
  code-shape decision only and contributes no segment either way.
- W-07 and W-12 test the **final** fragment bytes, so a script whose trailing
  `export default _sfc_main;\n` was removed by pass 2 and now ends without LF receives W-07's newline.
- W-18 has no trailing newline and is the module's last write.

**Structural facts the manifest establishes**, used by §6.3 and §6.4:

- **F-a.** Every B write begins at column 0, except W-07 and W-12, which write a single LF and
  therefore occupy no column at all (§2.1).
- **F-b.** Every B write ends with LF except W-18, which is the module's last write.
- **F-c.** Consequently W-06 and W-11 each begin at column 0 of a line: every write that can precede
  them ends with LF, or the module is still empty.

### 6.3 Placement

The assembler maintains a **write cursor** — the generated `(line, column)` of the next byte to be
written — updated by every write exactly as `advance_generated_position` updates a position
(`source_map.rs:577-591`). A fragment's **placement** is the cursor value when its first byte is
written: `(lineOffset, columnOffset)`.

A fragment segment at `(l, c)` is placed at

> `genLine = l + lineOffset`,
> `genCol = (l == 0) ? c + columnOffset : c`.

Placement is **derived from the write grammar as the assembler writes**. It is never supplied as an
input (§3.1), never recovered by scanning the generated output, and never reconstructed by
concatenating code first and computing offsets afterwards (BV0A.md:110-112).

**Derived invariant.** By F-c, `columnOffset` is `0` at both T1 and T3. An implementation may observe
this but must not assume it; the rule above is stated for all columns so it stays total if the write
grammar changes.

### 6.4 Boundary rules

**BR-1 — assembly-owned bytes carry no segment.** No B write contributes a segment (BV0A.md:87-90;
AMD-008:326-328).

**BR-2 — mapped fragment content contributes exactly its placed chained sequence** (§5, §6.3).

**BR-3 — the fragment-end boundary segment.** For each **contributing map**'s fragment (§5.8) at its
transition **T2** or **T4**:

> Emit one **sourceless** segment iff the fragment's final code **ends with LF** — equivalently, iff
> that fragment's newline patch (W-07 / W-12) does **not** fire. The segment is placed at
> `(bl, 0)`, where `bl` is the module line of the fragment's trailing empty line, and is positioned
> in the assembled sequence **after every placed segment of that fragment**. Otherwise emit none.

The condition is one predicate with one restatement, and both are properties of the fragment's final
bytes and of the write grammar, never of the composed segments. **It is not "the end cursor column is
zero."** Those are different predicates and they disagree on a real, legal input: a present fragment
whose `code` is `""` leaves the cursor at column 0 while its newline patch *does* fire, and case 4′ of
the proof below shows that fragment needs no boundary segment. An earlier revision stated all three as
equivalent; two of them are, and the cursor-column one is not.

A fragment that is not a contributing map emits no boundary segment (§5.8).

**BR-4 — no other boundary segment.** There is no fragment-start boundary segment, and none at the
module end (nothing follows W-18).

**BR-5 — the invariant BR-3 exists to guarantee.** *No generated position addressed by an
assembly-owned byte resolves to a source-bearing segment.*

**Proof over the manifest.** By F-a and F-b every B write's non-LF bytes occupy columns `0…` of lines
that write begins; by BR-1 no B write contributes a segment. So a violation requires a **fragment**
segment on a module line that also carries a B write's non-LF bytes at or after that segment's column.
The cases below are exhaustive over where such a line could come from, and every case names the
fragment's final byte, since that is what BR-3's predicate reads.

| # | Fragment geometry | Where the next B write's non-LF bytes land | Fragment segments on that line? | BR-3 | Verdict |
|---|---|---|---|---|---|
| 1 | any — a line no fragment covers | that line | none | n/a | `resolveAt` absent ✔ |
| 2 | any — the W-07/W-12 LF itself | no column at all (§2.1) | n/a | n/a | no position addresses it ✔ |
| 3 | non-empty, does **not** end with LF | line `bl+1`, outside the fragment's coordinate space (its max line is `bl`) | none | does not fire | ✔ |
| 4 | non-empty, **ends with LF** | column 0 of line `bl`, the fragment's own trailing empty line | possible, and only at column 0 | **fires**, at `(bl, 0)`, last in sequence | ✔ |
| 4′ | **empty** (`code == ""`) | line `lineOffset+1`, outside the fragment's coordinate space | none | does not fire | ✔ |
| 5 | any — a B write *preceding* a fragment on its line | requires `columnOffset ≠ 0` | — | n/a | impossible by F-c ✔ |

Case notes:

- **Case 2.** An LF occupies no column, so the end-of-line column of a fragment's last line addresses
  no byte at all — neither the fragment's nor the assembler's. Nothing there can be "assembly-owned",
  so there is nothing for a boundary segment to protect. An earlier draft of this specification, and
  the round-6 review finding it was written against, both treated this as a hole; it is not one.
- **Case 4.** `lineTable(code)` ends with an empty string, whose length is 0, so `U7.2` admits only
  column 0 on that line — every fragment segment that can land there, including a rule-(d) segment
  (§5.3), is at column 0. BR-3's segment is emitted after all of them, so by §2.3's last-wins rule it
  is the `resolveAt` winner for **every** column on line `bl`, including the columns the B write's
  bytes occupy. ✔
- **Case 4′ — the empty present fragment.** `lineTable("")` is `[""]`: one line, one in-bounds
  position `(0,0)`, which `U7.2` admits and which §5.1/§5.3(d) carry if `M` declares it, placed at
  `(lineOffset, 0)`. The fragment writes zero bytes, so the cursor is still `(lineOffset, 0)` — column
  0 — but `"".ends_with('\n')` is false, so W-07/W-12 **fires**, terminating a line that contains no
  characters whatsoever, and the next B write begins at `(lineOffset+1, 0)`, outside the fragment's
  coordinate space. No assembly-owned byte occupies any column on line `lineOffset`, so there is
  nothing for a boundary segment to protect and one would fail the emission standard (§2.6). BR-3
  correctly emits none.

  This is precisely why the predicate is "ends with LF" and **not** "the end cursor column is 0". The
  latter fires here, and firing here is not merely redundant — it is **destructive**. The boundary
  segment would land at `(lineOffset, 0)`, the same coordinate as the fragment's own carried segment,
  and BR-3 places it *after* every segment of the fragment; by §2.3's last-wins rule the sourceless
  boundary would then shadow the carried segment, making a faithfully composed authored position
  unobservable. A rule written to prevent fabricated provenance would have silently dropped a real
  one.

  The case is admissible input, and it is also **constructible from the rewrite passes themselves**:
  if a script's pass-1 output `C₁` consists of exactly one occurrence of pass 2's removal pattern, then
  `C₂` is `""` and the fragment BR-3 examines is empty. This specification does **not** claim that a
  template-only cell's synthetic script produces it — the real synthetic script block is non-empty and
  contains `const __sfc__` and `export default __sfc__`, and an earlier revision of this note wrongly
  implied otherwise. The DTO schema places no non-emptiness constraint on `code` (§3.3), so layer 1
  must be total over it either way.
- **Case 5.** Were `columnOffset ≠ 0`, the preceding bytes on that line would be assembly-owned and
  carry no segments by BR-1, and the only segment a previous fragment could have contributed there is
  its own BR-3 boundary segment, which is sourceless. This is why no fragment-**start** boundary
  segment is needed, and it also fixes the tie at the fragment's own first byte the right way round: a
  lookup at `columnOffset` must resolve to the fragment's own first segment, which is what happens
  when no start-boundary segment competes with it.

**Why BR-3 fires unconditionally in case 4, and how that squares with §11.2.** Case 4 is unreachable
for maps produced by `CodeTransform::generate_map`: a source-bearing token on a trailing empty line
would need an `Original` or `Overwritten` chunk beginning one past the final LF, impossible because
such a chunk must carry bytes (CT-4, CT-6's terminal-LF filter). So for every CT-producible input the
boundary segment changes `payloadAt` nowhere — the same property §11.2 uses to *exclude* two other
token kinds. The two rules nonetheless reach opposite conclusions, and under one standard, not two,
because §2.6's existential quantifier ranges over **admissible** inputs rather than CT-producible ones.

**The witness that admits BR-3.** Take a contributing fragment whose code ends with LF — say
`"function render() {}\n"` — whose map declares one source-bearing segment at `(1, 0)`, the trailing
empty line. That input is admissible: `U7.1` accepts line 1 because `lineTable` has two entries, `U7.2`
accepts column 0 because that line's text is empty, and every other check in §4.3 passes. §5.3(d)
carries that segment to the fragment's end position, which placement puts at `(bl, 0)`.

- **Without** BR-3's segment: for every column `c` on module line `bl` — including the columns the next
  B write's bytes occupy — `resolveAt` yields that carried segment, so `payloadAt(bl, c)` is its
  authored tuple. BR-5 is **false**.
- **With** it: BR-3 places a sourceless segment at `(bl, 0)` after the carried one, so by §2.3's
  last-wins rule it is the applicable segment at every column of line `bl`, and `payloadAt(bl, c)` is
  `Unmapped` for all `c`. BR-5 is **true**.

`payloadAt` differs between the two for this input, so §2.6's existential test **passes** and the rule
is admitted; and because the standard quantifies universally over instances, it then fires for every
fragment its syntactic condition selects, not only for the witnessing one. Contrast §11.2's excluded
tokens, for which no such witness exists at all — §11.2 case 2 proves the impossibility rather than
merely failing to find one. That is the same standard, applied once, producing both answers.

### 6.5 Fragment line ranges are disjoint

When both fragments are present, W-10 writes an LF between them, and the script's own final LF (its
own or W-07's) precedes it. The template's `lineOffset` is therefore at least one greater than the
script's last coordinate line, so no module line carries segments from both fragments. §5.5 rule 5's
concatenation is consequently also generated order.

---

## 7. Output artifact schema

### 7.1 What is compared

BV0A's exit compares the **complete decoded map artifact**, field for field and position for position,
including the exact ordered sequence of segments (AMD-008:118-126, :296-312):

```
MapArtifact := {
  version        : 3                                  // always present, always 3
  file           : ABSENT                             // §7.2
  sourceRoot     : string | ABSENT                    // §7.5
  names          : array<string>                      // always present, possibly empty
  sources        : array<string>                      // always present
  sourcesContent : array<string|null> | ABSENT        // §7.4
  ignoreList     : array<uint32> | ABSENT             // §7.3
  mappings       : string                             // §7.6
}
```

together with the decoded segment sequence of `mappings`. A multiset or sorted comparison of that
sequence is forbidden.

**The member names above are the COMPARED artifact's logical field names**, not JSON keys. The
comparison is over the decoded object, so a field is identified by which of §7.2–§7.6's rules produced
it, not by the string an encoder happens to write for it. This matters for exactly one field: the
ignore list is the logical member `ignoreList` here, while the in-tree encoder writes the JSON key
`x_google_ignoreList` (§7.8). Those are the same field.

**JSON member order is NOT part of the compared artifact, and is not layer-1 algebra.** AMD-008 is
explicit that "the compared artifact is the complete decoded map object" (AMD-008:127-128), and a
decoded map object has no member order. The independent JavaScript reference is therefore **not**
required to serialize in any particular order — it is not required to serialize at all; it produces the
decoded artifact, which is the thing compared. Two artifacts equal field for field and position for
position are equal however either side chose to spell them.

Member order matters in exactly one place, which is a **production serialization** property and is
recorded in §7.8 rather than here, because conflating the two is how a comparison contract acquires a
byte-level requirement it does not have.

### 7.2 Generated-side metadata is dropped; source-side metadata is preserved

The governing principle:

> Metadata that describes the **generated document** is dropped, because the document it described no
> longer exists. Metadata that describes the **sources table** is carried, either unchanged or with
> its indices remapped.

- **`file` — ABSENT.** Both fragment maps set `file` to the SFC filename
  (`crates/verter_compiler/src/compile/mod.rs:1004-1008`, `:1259-1263`), which is not the assembled
  module's identity. Inheriting it would be a false claim; choosing a new one would create a public
  contract BV0A does not own (§1.3). An input `file` is ignored.
- **`debugId` — ABSENT.** Same class as `file`; ignored.
- **Unknown / extension members — ignored, never inherited.** In particular
  `x_verter_helper_preamble_end` (`source_map.rs:601-635`) is a generated-side IDE boundary; the
  runtime script and template maps are produced through `generate_map_json`, not
  `generate_map_json_with_preamble` (`compile/mod.rs:1008`, `:1264`; the preamble variant is the TSX
  path at `compile/mod.rs:1671`), so it cannot appear on a real input, and it is dropped if it does.
- **`sourceRoot` — source-side, §7.5.**
- **`ignoreList` / `x_google_ignoreList` — source-side, §7.3.**

### 7.3 Ignore list

Every contributing map's ignore-list entries are carried, each shifted by that fragment's source-table
base offset (§7.4), in contribution order and, within a fragment, in the fragment's declared order.
The member is present in the artifact iff the resulting list is non-empty. Entries are already
validated in bounds by `U6.3`. Where an *input* map declares both spellings they must be deep-equal
(`U1.7`).

**The compared artifact's member is named `ignoreList`** (§7.1). The `x_google_ignoreList` spelling is a
*wire* fact — the literal JSON key an encoder writes — and belongs to serialization, not to the
composition algebra; §7.8 records it. Layer 1 fixes the logical field and its contents; the JSON key a
producer writes is a serialization convention, and two artifacts equal field for field are equal
whichever key either side spelled it with.

**Ignore status is a property of a ROW, not of a path.** The v3 ignore list indexes `sources` by
position, so under §7.4's append it can happen that one spelling occupies two rows of which only one is
ignored. The artifact then says exactly what its inputs said — *this* map's view of that source is
ignored, *that* map's is not — and a consumer that reads the list as the artifact defines it, by row,
gets a complete and non-contradictory answer. A consumer that first collapses rows by path is making an
identity inference the artifact does not assert, and the ambiguity it then meets is its own. §7.4's
`DECISION` D-5 addresses the case directly, because it is the strongest argument available for merging
rows instead.

**`DECISION` D-4.** The ignore list is **carried and remapped**, not rejected. Rationale: under §7.4's
append composition the remap is a pure index shift with no merge semantics, so it is trivially
specifiable and trivially agreed on by two implementations; carrying it drops no declared fact, and
rejecting a map over metadata composition can faithfully carry would hard-fail a cell for no reason.
Rejected alternative: reject any non-empty ignore list as uncomposable. That was revision 1's rule,
and it had a structural defect an adversarial review found — it lived in a cross-fragment stage that
never ran for a single-fragment compile, so the very silent drop it existed to prevent was exactly
what happened to a script-only map. Fixing the reachability made the remaining choice between
"reject everywhere" and "carry everywhere", and carrying is the one that discards nothing. This
decision is unreachable for current producers either way: the in-tree serializer always constructs
the map with `x_google_ignore_list: None` (`oxc_sourcemap-7.0.0/src/sourcemap.rs:53`, reached from
`source_map.rs:383-392`).

### 7.4 Source and name tables: stable append, no deduplication

Tables are composed in **contribution order**: the script fragment's rows first, then the template
fragment's rows. Within a fragment, rows are taken in the order the fragment's map declares them.

- `sources` is the concatenation of each contributing map's `sources`, verbatim.
- `names` is the concatenation of each contributing map's `names`, verbatim.
- A fragment's **base offsets** are the lengths of the tables accumulated before it: script's are
  `(0, 0)`; template's are `(|script.sources|, |script.names|)`, or `(0, 0)` when the script
  contributes no map.
- Every chained segment's `srcIdx` and non-null `nameIdx` is increased by the base offset of the
  fragment that produced it. Ignore-list entries are shifted by the same source base offset (§7.3).
- **No row is merged, dropped, reordered, or rewritten.** Two fragments declaring the same spelling
  contribute two rows. A row no segment references is still contributed.
- `sourcesContent` is the parallel concatenation: for each contributed row, the contributing map's
  `sourcesContent` entry at that row's local index when that map declares the member and the entry is
  a string, otherwise `null`. **The member is present iff at least one entry is non-null**, matching
  the in-tree serializer, which emits it iff any entry is `Some`
  (`oxc_sourcemap-7.0.0/src/encode.rs:20-32`, `:138-146`).
- `names` is always present, possibly empty. On real inputs it is always empty: the producer
  constructs the map with an empty names vector (`source_map.rs:383-392`, passing `Vec::new()`) and
  all nine `Token::new` call sites in `source_map.rs` pass `None` for the name id.
- `sources` may be `[]`, which can occur only when every contributing map is itself table-less (seed
  vector F7's synthetic empty script is the real instance); such a map necessarily has no
  source-bearing segment, since `U6.1` would otherwise have rejected it.

**`DECISION` D-5 — no deduplication.** Revision 1 of this document specified deduplication by exact
`(spelling, content)` row identity. That is **reversed here**; the reasoning, including the argument
that carried revision 1, is recorded in full because this is the single most consequential decision in
the document.

*The case for deduplication* (revision 1's): both fragment maps are produced with `include_content:
true` over the same filename (`compile/mod.rs:1004-1008`, `:1259-1263`), so append places the entire
authored SFC source **twice** in every two-fragment map — a real size cost on a shipped artifact — and
BV0A.md:19-22 says composition "neither invents, drops, reorders, **duplicates**, nor perturbs"
authored source spellings, which can be read as forbidding a spelling from appearing twice in the
output when each input declared it once.

*The case against, which prevails.* Three arguments, the first two raised by adversarial review:

1. **The charter's "duplicates" is about a carried fact, not about two declarations.** The subject of
   that sentence is the authored data a segment carries; "duplicates" forbids composition from
   emitting a second copy of one carried fact. Two fragments' independently declared table rows are
   two facts that happen to be equal, not one fact copied. Duplicate `sources` rows are ordinary,
   legal v3. Revision 1 over-read the sentence.
2. **Merging is itself an assertion, and revision 1 conceded it.** Revision 1 refused to merge a row
   with content and a row without, "because merging them would assert that the missing content is the
   same" — which concedes that merging is an assertion about declarations rather than a neutral
   quotient. The identical-pair merge is a smaller assertion, but it is the same kind of assertion,
   and BV0A carries declared identities **opaquely**: deciding that two independently declared rows
   are one row is a decision about identity that opacity withholds. Revision 1's answer — that the
   merge is a quotient by observational equivalence — is true only relative to a fixed notion of what
   is observable, and it stops being true the moment any per-row metadata is carried. §7.3 now carries
   exactly such metadata, so under revision 1's rules the deduplication policy's soundness would have
   depended on the ignore-list rejection revision 1 also specified — a coupling between two unrelated
   decisions that is itself evidence the merge rule was doing too much.
3. **Minimum authority for an interim block.** BV0A must not preselect B4's final representation
   (BV0A.md:107-109). Append decides nothing about row identity and needs no equality predicate; a
   canonicalization policy is exactly the kind of thing an interim composition should not be
   inventing. An independent round-6 review of AMD-008 made this point directly against mandating a
   canonicalization policy in this block.

*The strongest remaining argument for merging, and why it fails.* Both fragment maps commonly declare
the same path with the same content (`compile/mod.rs:1004-1008`, `:1259-1263`). Suppose the script's
map additionally declares `ignoreList: [0]` and the template's declares none. Under append the artifact
publishes that path twice, once ignored and once not, and a consumer that keys by path is left asking a
question neither input answered. That looks like the same "assertion about identity" D-5 uses against
merging — made by refusing to merge instead of by merging.

It fails on its own terms, because **merging does not avoid the assertion; it forces a stronger one.**
Confronted with two rows that differ in ignore status, a merging rule must either union the flags
(asserting the path is ignored, which the template's map denies), intersect them (asserting it is not,
which the script's map denies), or decline to merge rows whose ignore status differs — which is append,
for exactly this case, reached by a longer route. Append is the only one of the four that publishes
nothing beyond what an input declared: row 0 is the script's declaration, row 1 is the template's, and
each is exactly as its own map left it. §7.3 states the reading that makes this complete rather than
ambiguous — ignore status is per row, not per path — and the residual ambiguity belongs to a consumer
that collapses rows by path on its own initiative. The case is also unreachable for current producers,
which emit no ignore list at all (§7.3's `DECISION` D-4).

The size cost is real and is accepted: it is a quality property of an interim artifact that did not
exist at all before BV0A, and reducing it is B4's to do with the identity system it owns. The in-repo
precedent revision 1 cited for deduplication — the harness's official-golden composer, which keys rows
by `` `${source}\0${content}` `` (`src/sourcemap.mjs:186`) — is **not** a precedent for exact row
identity and is not relied on here: that key interpolates content into a template string, so an absent
content and a literal content of `"null"` collide.

### 7.5 `sourceRoot`

Normalise: `sourceRoot` is *absent* when the member is absent or JSON `null`, and `Some(s)` otherwise
(a non-string is `U1.7`). All contributing maps must normalise to the **same** value — both absent, or
both `Some(s)` with byte-equal `s` — or the input is `U8.1`. With a single contributing map the
condition is vacuous and that map's value carries through. With **zero** contributing maps — every
present fragment mapless (§5.8), the case §7.7 produces an empty artifact for — there is no value to
agree on and the composed `sourceRoot` is **ABSENT**; the condition is vacuous there too. Otherwise the
composed `sourceRoot` is the common value; the member is present iff it is `Some`.

This is the only faithful treatment: `sourceRoot` prefixes every `sources` entry, so dropping it while
copying spellings verbatim would silently change every declared source identity, and folding it into
the spellings would perturb spellings the charter requires to be carried unchanged. The oracle reads it
as part of source identity (`src/mapping-oracle.mjs:1148-1154`).

`""` is `Some("")` and is a distinct declared value from absent: this specification does not interpret
`sourceRoot`, so it does not decide that an empty root is the identity root.

### 7.6 Canonical `mappings` encoding

`mappings` encodes the assembled segment sequence **in sequence order**, never re-sorted:

- A generated line advance emits one `;` per line crossed; segments on the same line are separated by
  `,`; a generated line with no segments contributes an empty group.
- Within a line the first segment's column field is its absolute column; subsequent fields are deltas
  against the running accumulators `genCol` (reset to 0 at each line), `srcIdx`, `srcLine`, `srcCol`,
  `nameIdx` (carried across lines).
- A sourceless segment encodes exactly one field; a source-bearing segment encodes four, or five with
  a name.
- Encoding **stops after the last segment-bearing line**: no trailing `;` group is emitted beyond it.
- Two segments at the same coordinate encode with a zero column delta, which the decoder reads as an
  additional segment rather than a replacement (`src/sourcemap.mjs:80-103`), so sequence order survives
  a round trip.

This matches the in-tree serializer (`oxc_sourcemap-7.0.0/src/encode.rs:186-265`). It also matches the
accepted harness encoder (`src/sourcemap.mjs:108-143`) on every sequence this specification can
produce: that encoder sorts by `(genLine, genCol)` first (`src/sourcemap.mjs:110`), which is a no-op on
a non-decreasing sequence, and the sort is stable so equal-coordinate order survives. `U3.6` and §5.5
are what make that true; an implementation must not rely on the sort to impose order.

### 7.7 Result shape and the map-disabled case

The assembler returns one result carrying the assembled code and an optional map — the code and the map
from which it was generated, together (BV0A.md:202-205). Today the Vue path returns a bare `String` and
both callers observe no map for it, because `compiled.main.source_map` is empty for Vue
(`virtual_file_pipeline.rs:3010-3037`, `:3445-3455`): that absence is the gap BV0A closes.

When `sourceMapRequested` is `false`, the result carries **no map** — not an empty map, not a map with
an empty `mappings`. That absence is asserted positively, never by omitting the check
(AMD-008:313-315). When validation fails, there is no successful result at all (§4.2).

**When `sourceMapRequested` is `true`, a map is always produced, even with zero contributing maps.**
A module whose fragments are all present-but-mapless — a styles-only SFC, or a cell where every present
fragment is synthetic — yields the artifact `{ version: 3, names: [], sources: [], mappings: "" }`,
with `sourceRoot`, `sourcesContent` and the ignore list all ABSENT. It is not degraded to "no map":
AMD-008:296-299 requires a map-enabled cell to return code and a map together, and an empty map is the
truthful artifact for a module none of whose bytes carry a declared mapping. (BF2's oracle fails such a
map on its `source-identity` rule — `src/mapping-oracle.mjs:1144` — but BF2's mapping verdict is
excluded from BV0A's gate, and BV0A's own gate is artifact equality.)

### 7.8 Production serialization (not part of the compared artifact)

AMD-008:299-307 additionally requires production's own serialization to be **deterministic across
repeated identical invocations**, because production's `map_hash` is computed over raw serialized
bytes. That is a property of one implementation's encoder, not of the composition algebra and not of
the equality comparison (§7.1).

For the record, the in-tree serializer emits the members in the order `version`, `file`?,
`sourceRoot`?, `names`, `sources`, `sourcesContent`?, `x_google_ignoreList`?, `mappings`,
`debugId`? — the last one **after** `mappings` (`oxc_sourcemap-7.0.0/src/encode.rs:119-163`, with the
`debugId` arm at `:158-161`). Under §7.2 both `file` and `debugId` are absent from every artifact this
specification produces, so the conforming reduced order is `version`, `sourceRoot`?, `names`,
`sources`, `sourcesContent`?, `x_google_ignoreList`?, `mappings`. `debugId` is inventoried here rather
than omitted, so that the citation describes the encoder as it is and the reduction is visibly a
consequence of §7.2 rather than an incomplete reading. A conforming production implementation satisfies
the determinism exit by serializing deterministically; it does not have to match this order, and the
reference does not have to match it at all.

**The ignore list's JSON key is a serialization fact.** The compared artifact's field is `ignoreList`
(§7.1, §7.3); the in-tree encoder writes it under the key `x_google_ignoreList`, the only spelling it
emits. A producer writing either key emits the same compared artifact, because the comparison is over
the decoded object. On the INPUT side both spellings are accepted and must agree (§4.3 step 1.15,
`U1.7`) — that is a separate, decoding-side rule and does not make the two keys two fields.

---

## 8. Provenance

Provenance is composition-time bookkeeping (BV0A.md:66-70, AMD-008:253-257):

- Every ingested segment and every emitted segment is tagged at ingestion with its origin — `Script`,
  `Template`, or `AssemblyBoundary`.
- The tag **survives** rewriting, chaining, placement, and table remapping.
- The tag is **never inferred** from final coordinates or spelling.
- The tag is **never serialized**: no member of §7.1 carries it.

Under the manifest, `AssemblyBoundary` is attached only to BR-3's boundary segments; no other
assembly-owned byte produces a segment.

---

## 9. Relationship to the layer-2 seed vectors

The seed artifact is explicitly incomplete and non-normative pending this freeze
(`vectors/assembled-map-composition.vectors.json:2`, `:5`). Applying this specification to it:

| Vector | Level | Verdict |
|---|---|---|
| V1 | chain (single fragment, no assembly modelled — its `expected.code` is the post-rewrite fragment code, carrying none of §6.2's assembly writes, so BR-3 is outside what it asserts) | Reproduced exactly (3 segments). |
| V2 | chain | Reproduced exactly (5 segments, including the sourceless `(1,0)`). |
| V3 | chain | Reproduced exactly (2 coincident segments, in wire order). |
| V4 | assembly (two fragments, derived placement) | Tables reproduced exactly; chained segments reproduced exactly; **the expected SEQUENCE is incomplete** — it states 3 of the 5 segments this specification produces, omitting both BR-3 boundary segments. §9.1. |
| V5 | chain | Reproduced exactly (3 segments; UTF-16 columns 0, 11, 20). |
| V6 | chain | Reproduced exactly (3 segments). Its own recorded rework — it is non-discriminating for CR handling — is unrelated to this specification and still owed. |
| V7 | chain | Reproduced exactly (4 segments; the barrier holds at both lookups). |
| F1–F7 | — | Each is a straightforward instance of a §4.4 sub-code: F1 → `U1.1`; F2 → `U2.3`; F3 → `U1.3`; F4 → `U3.4`; F5 → `U6.1`; F6 → `U5.1`; F7 → composed, via §3.4's present-but-not-authored rule. |

Revision 1 of this document reported V4 and V6 as materially wrong. That report followed from two
revision-1 decisions that are reversed here — full `generate_map` token carriage (§11.2) and table
deduplication (§7.4) — and it no longer stands. Recording that plainly matters: a layer-1 draft that
declares reviewed layer-2 content defective, on the strength of decisions its own review then reverses,
is exactly the failure mode the layer split exists to catch, and it was caught here in the intended
place.

### 9.1 The one genuine gap: V4's expected sequence is incomplete

**V4's expected segment sequence is not what this specification produces, and §9's table says so
rather than calling it exact.** V4 is the only seed vector that models assembly placement — its
template segment lands at line 2 only because the assembler's separator write (W-10) puts it there —
so BR-3 applies to it. Both of its fragments are contributing maps whose final code ends with LF, so
BR-3 fires for both:

| From | Final code | Ends LF | BR-3 |
|---|---|---|---|
| script | `"const _sfc_main = {}\n"` (after both passes) | yes | sourceless segment at `(1, 0)` |
| template | `"function render() {}\n"` (placed at line 2) | yes | sourceless segment at `(3, 0)` |

The full sequence is therefore five segments — `(0,6)`, `(0,15)`, `(1,0)` sourceless, `(2,9)`,
`(3,0)` sourceless — where V4 states three. Its `sources`, `names` and its three chained segments are
each exactly right; the sequence as a whole is not complete.

Layer 2's completion must decide explicitly whether V4 becomes a complete assembly vector, in which
case both boundary segments belong in its expectation, or is re-scoped to the chain level like V1–V3
and V5–V7, in which case its `placement` input and its line-2 template segment go with it. Either way,
at least one layer-2 vector must pin BR-3, since no current vector does; the seed's own `knownGaps`
already records "an assembly-scaffolding boundary segment" as unpinned. And at least one must pin the
empty-present-fragment geometry of §6.4 case 4′, which no current vector reaches — F7 is the closest,
and the seed's own `knownGaps` already records that F7's empty synthetic script is a simplification
rather than the real block, which is non-empty.

---

## 10. Coverage

### 10.1 Against AMD-008's umbrella description (AMD-008:137-141, :150-158)

| Required | Section |
|---|---|
| the exact canonical output schema | §7.1–§7.6 |
| the exact chaining/transform algebra for both authorized rewrites | §5 |
| the exact rules for assembly-owned sourceless boundaries | §6.4 |
| the pre-assembly input DTO schema | §3 |
| the validation order and the exhaustive rejection taxonomy | §4 |
| equal-coordinate ordering and collision policy | §5.5 |
| an exhaustive manifest of the real assembler's write sites | §6.2 |
| every transition between mapped fragment content and assembly-owned synthetic content | §6.1, §6.2 (T1–T4), §6.4 |

### 10.2 Against the resolution gate's completeness check (`debt-layer1-gate-authority.md`, check 1)

| Named gap | Section | Resolved as |
|---|---|---|
| output field presence/policy | §7.1, §7.2, §7.3, §7.4, §7.5 | Per-member presence rule for all eight members, plus the generated-side/source-side metadata principle. |
| table merge/deduplication/remapping rules | §7.3, §7.4 | Stable append, no deduplication, contribution order, per-fragment base-offset remap of `srcIdx`, `nameIdx` and ignore-list entries, unused rows retained (`DECISION` D-5, D-4). |
| boundary placement behavior | §6.3, §6.4 | Placement derived from the write cursor; BR-1…BR-5 with a syntactic end-boundary condition and a five-case proof of BR-5 over the manifest. |

### 10.3 Against the seed artifact's `knownGaps`

| `knownGaps` item | Answered by |
|---|---|
| expected outputs are a partial projection, not the complete artifact | §7.1 defines the complete artifact. |
| V6 is non-discriminating for CR handling | §2.1 fixes CR-retaining line tables; the vector's own rework is still owed (§9). |
| F7 models the synthetic script as empty | §3.4 makes the outcome depend on the authored inventory, not on the synthetic block's contents, so a non-empty synthetic script composes identically. |
| a mid-line removal | §5.3(c) is stated per chunk, independent of column; the resume segment fires at the removal's generated position under §5.3(a). |
| a source-bearing old-end transition | §5.3(a): the resume segment carries `lookup(chunk start)`, source-bearing whenever an applicable segment exists on that line at or before it. |
| two distinct segments strictly inside a rename range | §5.5 rule 2: both dropped, one `Overwritten` segment emitted. |
| multiple same-line rename replacements | §5.3 walks chunks in order; each replacement is its own `Overwritten` chunk with its own segment and its own resume segment. |
| coincident-token ordering at a rewrite boundary | §5.5 rules 1–3 and the worked N-ary case. |
| an assembly-scaffolding boundary segment | §6.4 BR-3; §9.1 notes no vector pins it yet. |
| one vector per `UncomposableInputMap` variant (missing required map, malformed table container vs row, ignoreList shape/index, accumulator underflow, sourceRoot type, incompatible metadata) | §4.2 (`MissingRequiredInputMap`), §4.4 `U1.5`/`U1.6` vs `U4.1`–`U4.3` (container vs row), `U1.7` + `U6.3` (ignore list shape and index), `U3.5` (accumulator), `U1.7` + `U8.1` (`sourceRoot` type and conflict). |
| the DTO is not a frozen schema | §3.3, §3.5. |

---

## 11. Decisions, resolved oppositions, and scope answers

### 11.1 The `DECISION` register

| # | Decision | Section | Opposition resolved in |
|---|---|---|---|
| D-1 | A decreasing same-line generated column is rejected (`U3.6`) | §4.4 | §4.4, in the decision block |
| D-2 | A duplicate JSON object member is rejected (`U1.8`) | §4.4 | §4.4, in the decision block |
| D-3 | Only `Overwritten` and resume tokens are carried from the pass map | §5.3 | §11.2 |
| D-4 | The ignore list is carried and remapped, not rejected | §7.3 | §7.3, in the decision block |
| D-5 | Tables are a stable append with no deduplication | §7.4 | §7.4, in the decision block |
| D-6 | The fragment-end boundary segment is syntactically conditioned; there is no start boundary | §6.4 | §11.3 |
| D-7 | Input JSON numbers are binary64 values under round-ties-to-even, not exact decimals, for the domain bound and for every later numeric predicate | §4.5 | §4.5, in the decision block |
| D-8 | `U8.1`'s `fragment` attribution is `template` — decided for the present two-fragment DTO only, not claimed to generalise | §4.3 | §4.3, in the decision block |
| — | Input domains are BOUNDED rather than fully specified: precondition P1 on embedded DTO strings, and the interoperable JSON domain on input maps | §3.5, §4.5 | §11.6 item 1 |
| — | Validation is fail-fast: the first failure in §4.3's total order is the single reported outcome | §4.1 | — |
| — | Map requiredness is authored **and** present | §3.4 | §3.4, inline |
| — | `sourceRoot` must agree across contributing maps; `""` ≠ absent | §7.5 | — |
| — | `sourcesContent` is present iff at least one merged row carries content | §7.4 | — |
| — | With zero contributing maps a requested map is still produced, empty | §7.7 | — |
| — | `file`, `debugId` and unknown members are dropped | §7.2 | — |
| — | The compared artifact's ignore-list field is the logical name `ignoreList`; `x_google_ignoreList` is a wire key, not a second field | §7.1, §7.3, §7.8 | — |
| — | `sourceMapRequested` and `authored` are DTO fields | §3.3 | — |

D-3, D-4 and D-5 all reverse revision 1; D-6's predicate was corrected in revision 3; D-7 was registered
in revision 5 for a rule §4.5 had already stated but had not surfaced here. The reversals and
corrections are recorded rather than quietly applied, because a reader comparing revisions is entitled
to see which arguments moved.

Three rules are not `DECISION`s but carry comparable weight and are listed so they can be found: the
**emission standard** (§2.6), which every emission rule is derived from and which reconciles D-3 with
BR-3; the **mapless present fragment** case (§5.8); and **BR-3's proof obligation** (§6.4), whose case
table is what makes BR-5 checkable rather than asserted.

### 11.2 `DECISION` D-3 — which pass tokens are carried

§5.3 carries, from each pass, exactly the `Overwritten` chunk tokens (CT-8) and the own-start token of
each `Original` chunk that resumes surviving text after a replacement (CT-5 at that chunk's start). It
does **not** carry the own-start token of the first chunk, nor the interior line-start tokens of CT-6.

*The case for carrying the full set* (revision 1's): the charter calls the rewrites "real
`CodeTransform` code-and-map transforms ... each driving both output code and output map"
(AMD-008:244-248), and AMD-008 §1 item 3 identifies "a second model of the same operation" as the
defect the amendment exists to remove; filtering the transform's own token set could be read as such a
second model. The seed artifact's stated derivation bases also cite the unconditional own-start push
and the terminal-LF filter as load-bearing.

*The case against, which prevails.* The disputed tokens are provably **inert**, and adding an inert
segment to an artifact is an addition the charter's no-invent language does not authorise and no
correctness requirement demands:

1. **They are always sourceless or duplicates.** An interior line-start token, and the first chunk's
   own-start token, sit at a position `p`. If `M` declares a segment at `p`, the token would carry that
   segment's payload exactly and §5.3's suppression rule would drop it as a duplicate — nothing is
   added at all. If `M` declares none at `p`, then — because `p` is at column 0 of its own generated
   line for a line-start token, and at the fragment's very first position for the first-chunk token —
   there is no applicable segment at or before `p` on that line, so `lookup(p)` is absent and the token
   would be **sourceless**. There is no third case.
2. **In the sourceless case they change `payloadAt` nowhere, for any admissible input.** This is the
   step §2.6's standard actually requires, so it is proved rather than asserted. Let `T` be the
   candidate sourceless segment at `p`. Adding a segment can only change `payloadAt(q)` for `q` on
   `p`'s line with `q.column ≥ p.column`, and only where `T` becomes the new last-applicable segment —
   which requires that no other emitted segment lies in `(p.column, q.column]`. So the only positions
   at risk are those whose payload, without `T`, comes from an emitted segment at column `≤ p.column`.
   **There is no such segment**:
   - For an interior line-start token, `p.column` is 0, so a competing segment would have to sit at
     column 0 of that same generated line. The only emitted segments there could be a carried input
     segment at `p` — excluded by this branch — or a replacement or resume segment; but the generated
     line in question *begins inside* the `Original` chunk being walked (it starts immediately after an
     interior LF of that chunk's own content), so no other chunk contributes any byte or any segment to
     it.
   - For the first-chunk own-start token, `p` is the fragment's very first output position, so no
     emitted segment of that fragment can precede it at all. (After placement, a *previous* fragment's
     segment could only precede it on the same module line if `columnOffset ≠ 0`, which F-c excludes;
     and were it not excluded, §6.4 case 5 shows the only segment that could be there is a sourceless
     boundary segment, which leaves `payloadAt` `Unmapped` either way.)

   Therefore, without `T`, `payloadAt` at every position `T` could have won is already `Unmapped`
   (absent); with `T` it is `Unmapped` (sourceless). By §2.6's definition those are the **same**
   observable, so `T` changes `payloadAt` at no position of the pass's own chained output, for **no**
   admissible input.

   **The inertness survives the second pass**, which the proof needs because the script is chained
   twice. Pass 2 consumes pass 1's chained output as its `M`, so the question is whether a `T` that was
   inert in `M₁` can become observable in `M₂`.

   **What preservation does *not* follow from.** Order preservation alone (§5.5 rule 4) is **not**
   sufficient, and an earlier revision claimed an unrestricted induction on that basis. Rule (d)
   (§5.3) keys on **end-position-ness** — an offset equal to `len(T)` — not on relative order, and it
   can relocate an end-position segment onto a line that already carries content. Over
   `C = "xexport default _sfc_main;\n"` with one source-bearing segment at `(0,0)`, a sourceless
   segment added at the trailing empty line's `(1,0)` is inert there, yet pass 2's terminal removal of
   `[1,27)` leaves `C₂ = "x"` and rule (d) re-emits it at `(0,1)` — now to the right of a
   source-bearing segment on the same line, where it changes `payloadAt`. So a general "any inert
   segment stays inert across passes" claim is false, and is withdrawn.

   **The invariant that actually holds, and is all D-3 needs.** Both disputed token kinds are
   **strictly interior**: the token's image is never the last position the pass writes. The argument is
   a **remaining-byte** one, conducted entirely in the coordinate space actually being written, because
   a source-space inequality does not transfer across a pass that changes length — after one rename
   occurrence a later token's generated offset already differs from its source offset by +2.

   - **An interior line-start token** sits at content-relative offset `p` of its `Original{s,e}` chunk
     with `p < content_len` (CT-6's filter). So the byte `content[p]` exists, and an `Original` chunk's
     bytes are written to the output verbatim and in order (CT-14): that byte is written **after** the
     token's image, by this same chunk. The token's image therefore cannot be the last position this
     chunk writes, and so cannot be the pass's overall output end.
   - **The first-chunk own-start token** sits at its chunk's start. That chunk is an `Original` chunk
     and no `Original` chunk is ever empty (CT-4), so again at least one byte of it is written after
     the token's image.

   Both are also at **column 0** of their own generated line, in that same space: for the line-start
   token because the LF at content-relative `p − 1` is written verbatim immediately before its image,
   and for the first-chunk token because its image is the output's first position.

   That property is preserved by every subsequent pass, and in each branch the with-token and
   without-token outputs stay observationally identical:

   - **Inside a replaced range.** The token is dropped (§5.5 rule 2). Rather than claim the
     replacement's own segment is the *only* emission that could have observed it — it is not — the
     argument enumerates **every lookup that could resolve through the token's position**. §5.3 reads
     `M` in exactly three ways: `lookup(s)` for a non-empty replacement (rule (b)); `lookup(o)` at a
     resume emission point, where `o` is a replaced range's end (rule (a)); and the direct carry of
     `seg(o)` (rule (a)). The carry is irrelevant here — an offset inside a replaced range is never
     carried, with or without the token. For the two lookups, one argument covers both and every
     position they can be taken at:

     > The token is sourceless, at column 0 of its line, and is the only segment at-or-before column 0
     > on that line (the inertness premise). A lookup at any position `x` on that line therefore either
     > finds some other segment in `(0, x.column]`, which wins with or without the token, or finds
     > none — in which case the token wins **with**, yielding `Unmapped`, and nothing is applicable
     > **without**, yielding `Unmapped` as well. A lookup on any other line never sees it.

     So every such lookup resolves to the same `payloadAt` either way, whether it is the replacement's
     own `lookup(s)`, a resume `lookup(e)` at the range's end, or any other same-line lookup. Identical.
   - **At a replaced range's END — the resume position.** This is the line-merge case, where a removal
     that consumes the preceding LF makes the token's position land mid-line. It is covered, because
     the without-token branch emits an equivalent segment there anyway: with `seg(o)` empty, §5.3(a)'s
     resume clause emits a segment carrying `lookup(o)`, which the inertness premise makes sourceless —
     the same sourceless segment at the same position, so the outputs are byte-identical. With `seg(o)`
     non-empty, the resume segment is suppressed and the token would sit after that group; inertness
     forces the group's last member to be sourceless already, so appending another sourceless segment
     changes `payloadAt` nowhere.
   - **Carried anywhere else.** The token's offset lies in some `Original{s,e}` with `o < e`, so the
     byte at `o` exists and is written verbatim after the token's image by that chunk (CT-14) — the
     same remaining-byte argument as the base case, again in the space being written. The image is
     therefore **strictly interior again** in the new output. Its column stays 0, because merging its
     line into the preceding one would require a removal consuming the LF that starts its line, which
     puts it in one of the two branches above; absent that, the LF is still written immediately before
     its image. So nothing precedes it on its line and it remains inert.

   **Therefore rule (d) can never apply to either disputed token kind, in any number of chained
   passes** — it fires only at a fragment's own terminal offset, and strict interiority is preserved at
   every step. The counterexample above is outside this claim by construction: its segment is an
   end-position segment, which neither token kind can be. §2.6's existential test fails and the rule is
   excluded. ∎
3. **The two tokens this specification does carry are each required**, i.e. each passes §2.6's test.
   The `Overwritten` token is charter-mandated geometry and plainly changes `payloadAt` over the
   replacement. The resume token likewise: without it, surviving text immediately after a replacement
   inherits the replacement's own authored position (for a non-empty overwrite) or whatever lay to the
   left of the removed range (for a removal) — a real fabrication of provenance for text that was not
   replaced, and a real change in `payloadAt`.
4. **No second geometry, stated precisely.** The two token kinds this specification *does* carry from a
   pass sit at exactly the positions `CodeTransform` emits them at, with exactly CT-8's and CT-5's
   geometry, and carry the payload that token's original position resolves to. Carried input segments
   are a different thing and are described as such in §5.3: they are declared *input* positions
   composed with the pass's **chunk structure**, not `CodeTransform` tokens. So the accurate claim is
   not "the output is a subset of the pass map" — it is that **every geometric decision this
   specification makes is `CodeTransform`'s own chunk list**, and that it emits no pass token
   `CodeTransform` would not emit, at no position `CodeTransform` would not emit it. The "second model"
   hazard AMD-008 §1 item 3 names — two models of one operation that *disagree* — therefore cannot
   arise; its target was a bespoke offset/clamp formula producing *different* geometry.

The result is that every segment in an assembled map has a reason: it is a carried input segment, a
charter-mandated replacement segment, a resume segment that stops provenance bleeding rightward, or a
BR-3 boundary segment that stops it bleeding into assembly-owned bytes. That property is worth more
than mechanical identity with a helper's token list.

**Reconciliation with BR-3, which also emits sourceless segments.** The argument above and §6.4's
boundary rule are the same standard (§2.6) applied twice, not two rules that happen to disagree. The
difference between the token kinds is *for which inputs* they can matter:

| Emission | Changes `payloadAt` for some CT-producible map? | Changes `payloadAt` for **some** admissible input? | Emitted |
|---|---|---|---|
| first chunk's own-start token | no | **no** — proven by cases 1–2 above | no |
| interior line-start token | no | **no** — same proof | no |
| BR-3 boundary segment | no (§6.4's unreachability argument) | **yes** — §6.4's witness: a wire-valid map declaring a source-bearing segment on the fragment's trailing empty line, which `U7.1`/`U7.2` admit at column 0 | yes |

§2.6's standard quantifies existentially over admissible inputs, so it excludes the first two rows and
admits the third; and it quantifies universally over instances, so the third fires unconditionally
rather than only when the witnessing segment is present. Layer 1 is total over wire-valid input, not
over the subset today's producers happen to emit — which is exactly why "changes nothing for every map
`CodeTransform` can currently produce" is not sufficient grounds to drop a segment, and why the two
rows that *are* dropped needed the stronger proof over all admissible inputs.

Vector agreement is deliberately not part of this argument. Revision 1 rejected the narrower reading
partly on the ground that the wider one "corrects" two seed vectors; that was backwards — vector
agreement is evidence about what a previous author believed, not about what is correct — and the
argument above stands on its own.

### 11.3 `DECISION` D-6 — the boundary-segment condition

BR-3 emits a fragment-end boundary segment iff a **contributing map**'s fragment's final code **ends
with LF**, and emits no fragment-start boundary segment at all.

*The case for unconditional emission:* BR-5 should be a syntactic fact about the write grammar, not a
predicate over composed content, because a predicate that must be evaluated at a particular moment
against "the segments emitted so far" is exactly the kind of rule two independent implementations can
implement with different evaluation timing.

*Resolution.* That argument is accepted in full, and it is why revision 1's content predicate is gone:
revision 1 conditioned emission on `resolveAt(segments-so-far, …)` being source-bearing, evaluated
*before* the fragment's segments at T1/T3 and *after* them at T2/T4. BR-3's condition is a property of
the fragment's final byte, so BR-5's proof (§6.4) runs entirely over the manifest, and §2.6's emission
standard supplies the general principle: a surviving rule fires for every input its syntactic condition
selects, never only when it happens to matter.

Two subsequent corrections are recorded here because both are places this decision was, at some point,
stated wrongly:

1. **Revision 2 restated the condition three ways and claimed they were equivalent.** They are not.
   "The end cursor column is 0" is true for a present fragment whose `code` is `""` while the other two
   are false, and §6.4 case 4′ shows that fragment must **not** receive a boundary segment. Two
   independent round-2 reviews constructed that input from scratch and reached the same conclusion. The
   cursor-column formulation is deleted; "ends with LF" and "the newline patch does not fire" are
   genuinely equivalent — the patch's own condition is `!code.ends_with('\n')` (`compile.rs:144`,
   `:189`) — and either may be used.
2. **The rule applies to contributing maps only.** A present fragment with no map contributes no
   segments at all, so there is nothing on its lines for a boundary segment to protect; emitting one
   would fail the emission standard (§5.8).

The remaining difference from full unconditionality — BR-3 emits nothing when the fragment's code does
not end with LF — is not a weakening: §6.4 case 2 shows the position in question addresses no byte at
all, because an LF occupies no column, so there is nothing there to protect and emitting one would mark
a position inside the fragment's own last line as unmapped. The start boundary is likewise gone, with
§6.4 case 5 proving it protects nothing and fixing the tie at the fragment's first byte the right way
round.

### 11.4 Scope answer — a malformed DTO instance

A DTO instance that violates §3.3's schema (a negative `styleCount`, a `hmrStrategy` outside the three
variants, a missing or extra member) or §3.5's precondition P1 is **out of layer-1 scope**, and gets no
`UncomposableInputMap` family.

Rationale: the DTO is a serialization boundary that exists for the independent reference. Production
never receives one — it receives typed Rust values in which these malformations are unrepresentable
(`compiled.styles.len()` is a `usize`, `HmrStrategy` is a three-variant enum). Putting DTO validity into
the composition taxonomy would import a test-harness concern into a production contract and would make
the two implementations' failure taxonomies non-comparable, since only one of them can even observe the
failure. DTO instance validity is owned by the **layer-2 vector suite's own schema binding**, which
BV0A.md:34-37 already requires ("completed and schema-bound as a BV0A acceptance deliverable"); a
schema-invalid vector is a defective vector, caught at suite load, not a composition outcome.

### 11.5 Scope answer — what happens when `CodeTransform` changes

This specification is **self-contained and changes only by amendment**. The CT-1…CT-14 table is a
recorded snapshot of the cited implementation as of the tree named in the front matter, present so that
§5's rules are auditable against the code they were derived from. The **normative** content is §5's
rules, stated independently of any implementation.

Therefore: a later change to `CodeTransform`, `emit_mapped_content`, `oxc_sourcemap`, or any other cited
helper does **not** propagate into this specification. If such a change would make production's
behaviour diverge from §5, production must continue to conform to §5 — the composition may not silently
inherit a behaviour change from a shared helper — and the divergence is resolved by amending this
document, which is what "changeable only by amendment thereafter" (AMD-008:161-164) means. A bug found
in a cited helper is thus two separate pieces of work: fixing the helper, and (if the fix touches
composed geometry) amending this specification.

### 11.6 Genuinely open — deliberately not decided here

1. **DTO strings outside precondition P1.** §3.5 bounds the write grammar to printable ASCII for the six
   embedded strings, because Rust's `{:?}` escaping outside that domain is driven by a Unicode
   printability table that is a standard-library implementation detail. Every id in BF2's seed manifest
   is inside P1, so BV0A's exit is unaffected; extending the specification is a future amendment's work
   or B4's. (Revision 2 added a parenthetical claiming Rust's `\u{…}` escapes are invalid JavaScript.
   That was **wrong** — `\u{…}` is a valid ECMAScript unicode code-point escape — and it is deleted.
   P1's justification never depended on it: the reason to bound the domain is that reproducing Rust's
   printability and grapheme-extended tables in a second language is a Unicode-version-coupled hazard,
   which stands on its own.)
2. **The typed name and shape of the production result** is an implementation choice, not a semantic
   one. This document specifies what the map *is*, not the Rust type that carries it or how it reaches
   `CachedVirtualFile::source_map` (`virtual_file_pipeline.rs:3029-3048`).
3. **Rescope routing** (which of BF2 / BV0 owns a given root cause) is charter behaviour
   (BV0A.md:176-181) and is deliberately not re-specified, to avoid a second statement of a ratified
   rule.
4. **Multi-fragment futures.** This algebra is stated for the two mapped fragments the current write
   grammar produces. A third mapped fragment (style content, custom-block content, an IDE surface) would
   need §6.2's manifest, §6.3's placement, §6.5's disjointness argument and §5.5's rule 5 re-derived.
   That is B4/BV1 territory (§1.3), out of scope by construction.

Two items that were open in revision 1 are now closed and have been moved out of this list: the
map-disabled versus map-failed result shape (now stated in §4.2 and §7.7) and the status of DTO
instance validity (now §11.4).

---

## 12. Revision history

**Revision 8 — POST-FREEZE amendment, not a pre-freeze review round.** Revision 7 was adopted as
frozen (blob `0ea47424acfbd4913e11f16156baa597216c84fb`). Per AMD-008, changing frozen
layer-1 semantics after that point requires its own amendment, not a silent edit and not an
implementation's own judgment call — this revision is that amendment, prompted by exactly the
failure mode the freeze exists to prevent: the independent JavaScript reference and the production
Rust implementation, each built against revision 7 with zero visibility into the other, both hit
`U8.1`'s unspecified `fragment` attribution and each resolved it on its own, without a shared rule
to defer to. They happened to reach the same answer; that is a fact about two implementers'
instincts, not evidence the frozen text was complete. Change:

- **§4.3 step 2.1 gains an explicit `fragment` attribution rule for `U8.1`, registered as
  `DECISION` D-8**: the template, derived from the fixed script-then-template contributing-map order
  and the fact that `U8.1` is reachable only when both of the DTO's two map-carrying slots are
  contributing maps. No other rule, derivation, or prior `DECISION` is touched.
- **First-round independent review (2 mandates) found the rule itself correct but its
  future-fragment framing over-claimed.** The original text said the rule "requires no further
  amendment if a future charter ever admitted a third fragment kind"; one reviewer confirmed no
  admissible two-fragment input breaks the rule, but both flagged that a third mapped fragment
  makes "the later contributing map" genuinely ambiguous (first-mismatch vs. last-in-list diverge
  the moment more than one map can disagree with an established baseline) — exactly the kind of
  future-generalisation claim §11.6 item 4 already reserves for its own re-derivation. The
  future-proofing sentence is deleted; D-8 now decides only the present two-fragment DTO and says so
  explicitly. The "stage 1 establishes the script's `sourceRoot` as baseline" phrasing is also
  corrected to describe stage 2.1's sequential reading, since stage 1 only type-checks each map
  independently and does not itself write a baseline.

This revision requires its own independent review before it is adopted alongside revisions 1–7; see
the front matter's status line. Once adopted, both the independent reference and the production
implementation must be brought into conformance with the reviewed rule as stated here — not left on
whichever answer each had already guessed, even where, as here, the guesses matched (both
independently already produce `template`, confirmed during review).

**Revision 7** — two prose-precision corrections to §11.2's proof under a sixth independent round:
architecture `PASS`, adversarial `PASS`, conformance `FAIL` on two gaps in how the proof *argues* its
conclusion, with the conclusion itself independently confirmed correct and the withdrawn counterexample
confirmed correctly excluded. Neither change alters any rule or any derivation.

- **The strict-interiority argument mixed coordinate spaces.** It derived `s + p < e ≤ len` in
  **source** offsets and concluded the token's image is strictly before the **generated** output end;
  that inequality does not transfer across a length-changing pass, since one rename occurrence already
  shifts a later token's generated offset by +2. Replaced with a **remaining-byte** argument conducted
  in the space actually being written: CT-6's filter guarantees `content[p]` exists, and CT-14 writes
  an `Original` chunk's bytes verbatim and in order, so at least one byte is written after the token's
  image by that same chunk — hence the image is not the last position the pass writes. The first-chunk
  token gets the same argument from CT-4 (no empty `Original` chunk), the column-0 claim is re-derived
  the same way (the LF at `p − 1` is written immediately before the image), and the carry branch now
  reuses it explicitly rather than restating the numeric form.
- **The dropped-inside-a-replaced-range branch claimed exclusivity it does not have.** It said the
  replacement's own segment was "the only other emission" that could observe the dropped token; a
  resume lookup at the range's end, and any other same-line lookup, can also resolve through that
  position. The claim is dropped in favour of enumerating **every** way §5.3 reads `M` — `lookup(s)`
  for a non-empty replacement, `lookup(o)` at a resume point, and the direct carry of `seg(o)` — and
  showing one argument covers all of them: the token is sourceless at column 0 with nothing else
  at-or-before it on its line, so any same-line lookup either finds a nearer segment (which wins either
  way) or finds none (yielding `Unmapped` with the token and `Unmapped` without it).

**Revision 6** — one proof-level correction under a fifth independent round: adversarial `PASS`,
architecture `PASS_WITH_NOTES`, conformance `FAIL` on a single finding. All three reviewers landed
independently on the same paragraph — §11.2's cross-pass induction — and all three confirmed the
finding does not change D-3's conclusion. Change:

- **§11.2's cross-pass induction was stated too broadly and is re-scoped.** Revision 5 claimed
  order preservation (§5.5 rule 4) carried inertness across "any number of passes" for "any inert
  token". That is false as a general claim: rule (d) keys on **end-position-ness**, not relative order,
  and can relocate an end-position sourceless segment onto a line that already carries content, where
  it becomes observable. The counterexample is now stated in the text and the general claim withdrawn.
  In its place the paragraph states the narrower invariant D-3 actually needs — both disputed token
  kinds are **strictly interior** (offset `< len`) and at **column 0**, by CT-1 and CT-6's filter
  respectively — and preserves it inductively through three branches: dropped inside a replaced range;
  coincident with a resume position, where the without-token branch emits an equivalent sourceless
  segment anyway (this is the line-merge case); or carried elsewhere, where it stays strictly interior
  and at column 0. Rule (d) therefore can never apply to either token kind, so the counterexample is
  outside the corrected claim by construction. D-3's conclusion is unchanged.

**Revision 5** — targeted polish under a fourth independent round: adversarial `PASS`, architecture
`PASS_WITH_NOTES` (two narrow non-correctness findings), conformance `FAIL` (one narrow blocking
finding, the same root as one of architecture's). All three rounds converged that the composition
algebra is correct and stable; nothing in this revision changes any derivation. Changes:

- **§4.5 now fixes decimal→binary conversion exactly**: the conversion is IEEE-754 binary64 under
  round-ties-to-even, a number is in-domain iff that conversion is finite, and **every** later numeric
  predicate — integrality, equality, non-negativity, index comparison — operates on the converted value
  rather than the exact decimal lexeme. Specifying the rounding mode is also what makes the domain
  boundary determinate, since a magnitude just above the largest finite double converts to that double
  rather than to an infinity.
- Registered that choice as **`DECISION` D-7** with its rejected alternative (exact-decimal semantics),
  per §1.4's own method, and audited §11.1's register in full: six further genuine definitional choices
  that had been stated only in place — the bounded input domains, fail-fast validation, map
  requiredness, `sourcesContent` presence, the zero-contributor empty map, and the previously listed
  unnumbered rows — now appear there.
- **§11.2's impossibility proof now covers propagation across the second pass** rather than stopping at
  one pass's chained output, invoking §5.5 rule 4 (chaining preserves relative order) and the fact that
  every downstream consumer of `lookup` reads it through `payloadAt`, which collapses sourceless and
  absent identically.
- **§7.8's encoder inventory now includes `debugId`**, which the in-tree encoder emits after `mappings`
  (`oxc_sourcemap-7.0.0/src/encode.rs:158-161`); §7.2 makes it absent from every artifact here, so the
  reduced conforming order is unchanged and is now visibly a consequence of §7.2 rather than of an
  incomplete reading.

**Revision 4** — revised under a third independent round: architecture `PASS_WITH_NOTES` (contamination,
scope and citations clean again; one precision gap flagged), conformance `FAIL` (3 blocking, 2
non-blocking), adversarial/governance `FAIL` (3 blocking). The round's principal finding was reached
independently by **all three** reviewers. Changes:

- **§2.6's `observable` did not license the equivalence its own consumers rely on.** `resolveAt`
  returns distinguishable values for *absent* and *a present sourceless segment*, while §11.2 and §5.8
  both reason as though those were one outcome. §2.6 now defines the observable as **`payloadAt`** —
  the authored tuple when the applicable segment is source-bearing, and the single value `Unmapped` for
  both other cases — and derives that quotient from §5.4, from BR-5's own "source-bearing" phrasing,
  and from the accepted oracle's `inherited !== null && inherited.srcIdx !== null`
  (`src/mapping-oracle.mjs:1324-1325`). §11.2's exclusion and §6.4's inclusion were both re-derived
  from the corrected definition rather than re-asserted: the exclusion now proves that no emitted
  segment can precede the candidate on its line, so `payloadAt` is already `Unmapped` everywhere the
  candidate could win; the inclusion now states an explicit admissible **witness** and shows
  `payloadAt` differing with and without the segment.
- **`U6.1` had no sourceless guard**, so a literal reading rejected every 1-field segment and made the
  whole sourceless-barrier algebra — and seed vector V7 — unreachable. Step 1.22 and the `U6.1` row now
  guard on `srcIdx` being non-null, matching the guard `U6.2` already had.
- **The compared artifact's ignore-list field is `ignoreList`**, a logical field name; the
  `x_google_ignoreList` JSON key moved to §7.8 as a serialization fact, with §7.1 stating the
  distinction between logical field and wire key explicitly.
- **Added the interoperable JSON domain** (§4.5, sub-codes `U1.9`, `U1.10`, checked inside step 1.1):
  numbers must denote finite IEEE-754 doubles and strings must be well-formed Unicode, enforced
  regardless of what either language's native parser would do, because "parses as JSON" is not one
  thing across a JavaScript reference and a Rust producer.
- **Corrected the `CodeTransform`-subset overclaim** in §5.3 and §11.2 item 4: carried input segments
  are declared input positions composed with the pass's chunk structure, **not** `CodeTransform`
  tokens — with an empty `sourcemap_locations` set (CT-13) `generate_map` emits nothing at an interior
  input-segment position, and coincident segments have no token representation at all. Only the pass
  tokens carried are a subset; the accurate invariant is that every geometric decision is
  `CodeTransform`'s own chunk list.
- Notes: §2.6's "five kinds" is now an explicit five-item list; §5.3's payload-precedence rule cites
  §2.6 so "one standard, applied consistently" is literally true of the text; and the case-4′ note no
  longer attributes the empty-`code` case to a template-only cell's synthetic script — that block is
  non-empty. The case is retained as admissible input, and is additionally shown constructible from
  pass 2's own removal.

**Revision 3** — revised under a second independent round: architecture `PASS_WITH_NOTES` (≈105
citations independently re-verified, all confirmed; the three revision-2 reversals confirmed sound),
conformance `FAIL` (4 blocking findings, 2 citation corrections), adversarial/governance `FAIL` (4
findings, one of them the same core defect conformance found independently). Changes:

- **BR-3's predicate was wrong for an empty present fragment**, the defect both reviews constructed
  independently. Revision 2 stated three "equivalent" conditions; the cursor-column one is not
  equivalent and gives the wrong answer for `code == ""`, an input the DTO schema admits. (Revision 3
  additionally called it a real current input, attributing it to a template-only cell's synthetic
  script; revision 4 retracts that attribution — see below.)
  BR-3 is now one predicate ("the fragment's final code ends with LF", equivalently "its newline patch
  does not fire"), restricted to contributing maps, and BR-5's proof is a six-row case table with the
  empty-fragment geometry as its own row 4′ (§6.4, §11.3).
- Added the **emission standard** (§2.6) as the single rule all emission decisions derive from, and used
  it to reconcile D-3's exclusions with BR-3's inclusion under one principle rather than two
  (§11.2's reconciliation table): the excluded tokens are inert for *every* admissible input, BR-3's
  segment only for CT-producible ones.
- Stated the **mapless present fragment** as its own case (§5.8): no carried, replacement, resume,
  table, ignore-list or boundary contribution, with the emission standard as the reason.
- Resolved §5.1 against §5.3(d) for empty code: **rule (d) fires with an empty chunk list**, and
  deleted the false parenthetical restricting rule (d) to LF-terminated fragments — it is live for
  every fragment, and reading it as a guard drops legitimate segments.
- Mandated the **per-segment decode precedence** as three ordered phases (lexical/per-field → arity →
  accumulator application), so arity beats every accumulator property and range beats ordering (§4.3
  step 1.21).
- Moved **JSON member order** out of the compared artifact into a production-serialization note
  (§7.1, new §7.8): AMD-008 compares the decoded map object, so the reference need not serialize at
  all.
- Defined the **zero-contributing-map** case: a map is still produced when requested, empty and with
  `sourceRoot` ABSENT (§7.5, §7.7).
- Answered D-5's strongest remaining counter-argument — conflicting ignore status on a duplicated
  spelling — by showing that merging forces a *stronger* assertion than refusing to; added the
  per-row-not-per-path reading of ignore status (§7.3, §7.4).
- Corrected §9/§9.1's self-contradiction about V4: its expected sequence is 3 of the 5 segments this
  specification produces, and the summary table no longer calls it exact.
- Citation corrections: CT-8 now carries the same `source_id` guard as CT-5, with the advance noted as
  sitting outside it; deleted the false claim that Rust's `\u{…}` escapes are invalid JavaScript (they
  are valid ECMAScript unicode code-point escapes — P1's justification never rested on it).

**Revision 2** — revised under independent conformance review (7 findings) and adversarial/governance
review (6 blocking findings plus 3 unresolved decision oppositions); architecture review closed
`PASS_WITH_NOTES` on revision 1. Changes:

- Reversed three revision-1 decisions after their oppositions were argued rather than merely noted:
  pass-token carriage narrowed to `Overwritten` plus resume tokens (D-3, §11.2); the ignore list carried
  and remapped instead of rejected (D-4, §7.3); source and name tables changed to stable append with no
  deduplication (D-5, §7.4). §9 accordingly withdraws revision 1's claim that seed vectors V4 and V6
  were materially wrong.
- Replaced the boundary rule's content predicate with a syntactic condition, deleted the
  fragment-start boundary segment, and rewrote BR-5's proof as five explicit cases including the
  trailing-empty-line and no-trailing-LF geometries (D-6, §6.4, §11.3).
- Rewrote §4.3 as a single flat total order over every individual check, with element scan order inside
  every array and field order inside every segment; made stage 2 run over the contributing set at any
  cardinality, closing the single-fragment reachability hole.
- Added `U1.8` (duplicate JSON object member), `U3.6` (decreasing same-line generated column) and
  `U6.3` (ignore-list index out of table).
- Restated §5.3's rule precedence so that a replaced range is governed solely by rules (b)/(c), and
  added the worked N-ary coincidence case to §5.5.
- Rewrote §6.2 as an exact byte grammar with the two formatting functions, the two derived id
  renderings, and every conditional write spelled out; added precondition P1 to §3.5 to bound it.
- Answered the two scope questions explicitly (§11.4 malformed DTO instance, §11.5 helper changes) and
  stated in §4.2 that a validation failure yields no successful result at all.
- Corrected the `has_template` citation (`parse.rs:1347-1355`, independent of `has_script`), qualified
  CT-5 with its `source_id` guard, added the `genCol ≥ 1` guard to `U7.3` and the character-boundary
  lemma to §5.2, and withdrew the harness-composer deduplication precedent as inexact.

**Revision 1** — initial draft.
