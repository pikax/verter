# AT-2 — disposition consult, verbatim ruling

The prompt that produced it is reproduced in [`at2-disposition-prompt.md`](at2-disposition-prompt.md).
Seat: Codex `gpt-5.6-sol`, reasoning effort `high`, read-only sandbox. Unprimed: the prompt states
verified facts and asks for a disposition, and explicitly invites "your premise is wrong".

One editorial change was made to the text below: the absolute checkout-local paths inside the
citation links were rewritten to repository-relative form, so this record reads the same on every
machine. Nothing else — no word, no verdict, no line reference — was altered.

---

## Ruling

`AT-2`, as ratified, is not an evidenced genuine defect. Its factual claim is wrong: no current path has been shown to publish a product together with a genuine typed refusal.

There is a separate latent result-shape hazard at HostBacked construction site 3. That hazard should be explicitly reclassified and deferred to `BA0`; it must not continue masquerading as an observed defect.

### 1. Classification

A genuine typed refusal is `HostError::RuntimeSurfaceRefused`, carrying a stable diagnostic code and message ([types.rs](crates/verter_session/src/types.rs:2480)). `get_virtual_file` returns that as `Err`, not `Ok` ([virtual_file_pipeline.rs](crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1087)). The batch’s generic `Err(host_err)` arm publishes errors with empty code, map, and language ([host_compile.rs](crates/verter_session/src/host_compile.rs:812)). Therefore the actual typed-refusal path is atomic.

Site 3 instead handles an `Ok(response)` and flattens its error-severity diagnostics into `CompileBatchEntry.errors` while preserving its products ([host_compile.rs](crates/verter_session/src/host_compile.rs:742)). The known upstream producer is last-known-good serving, which the source explicitly says “is not a fresh runtime refusal” ([virtual_file_pipeline.rs](crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1755)).

Accordingly:

- The ratified claim “product together with a genuine typed refusal” is not a current defect.
- Site 3 is a genuine latent representation/construction hazard: the flat DTO can express product plus fatal-looking `errors`.
- Its public reachability is unresolved, not proven impossible, because the fact-empty residual remains `UNKNOWN`.
- The current conflict test is merely a green atomicity control. It cannot characterize AT-2; the record already admits that ([dispositions.md](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:35)).

The correct recorded disposition is therefore: amend `AT-2` to “latent HostBacked result-outcome construction hazard, reachability unproven,” retain `DEFER` and `BA0` if BA0 is empowered to introduce the immediate typed boundary. Reject the old finding text as an observed genuine defect.

### 2. Regression obligation and exact test

Charter item 6 applies only to “every genuine defect” ([BF3.md](docs/arch/refactor/rev11/charters/BF3.md:34)). After the proper reclassification, AT-2 does not create a defect-specific item-6 regression obligation.

BF3 still owes the charter’s separate, mandatory atomicity exit ([BF3.md](docs/arch/refactor/rev11/charters/BF3.md:51)). That should be a green, table-driven public regression that:

- Drives every demonstrated failure class through both `HostBacked` and `RuntimeRender`, where applicable: duplicate-canonical conflict, upsert failure, compile failure, other host failure/refusal, and panic.
- Includes an independently successful neighbouring entry.
- Proves each tested entry genuinely entered the intended failure class.
- Asserts every failing entry has no code, source map, or output language.
- Asserts its neighbour retains code and has no failure.
- Includes ordinary successful, warning-only, and stale-success controls so the test does not equate every diagnostic with refusal.

For the latent hazard, BA0’s acceptance should be structural. The sound model is an outcome sum such as:

```text
Success(Product)
StaleSuccess { last_good_product, diagnostics }
Failure(TypedFailure)
```

There must be no representable variant containing both `Failure` and `Product`. The existing flat `CompileBatchEntry { code, errors, ... }` does not provide that guarantee ([host_compile.rs](crates/verter_session/src/host_compile.rs:116)).

The discriminating conversion tests should drive:

1. A typed `RuntimeSurfaceRefused` and assert `Failure`, preserved diagnostic code, and no product.
2. A synthetic or real last-known-good `Ok` response containing sentinel code/map/lang and a new error diagnostic, and assert `StaleSuccess`: the last-good product remains available, the diagnostics survive, and it is not classified as a refusal/fatal `errors` outcome.
3. A mutation that maps either case to a mixed failure-plus-product outcome and must make the guarding test fail.

That avoids “fixing” intended stale serving by withholding its last-good product.

### 3. Ignored target and injection seam

An ignored target is legitimate only if its setup reaches the exact state it claims to test and the assertion fails on current behavior. An ignored Svelte batch test that presently fails only because RT-1 prevents Svelte classification is not independently discriminating for atomicity. A test that skips, returns early, or never reaches its atomicity assertion is a stub.

A wholly private `#[cfg(test)]` injection seam can remain within BF3’s zero-production-behavior ceiling if:

- Every seam declaration, field, and branch is compiled out of production.
- It changes no production API, layout, feature, or behavior.
- The test proves the seam was consumed.
- It is used only to expose and characterize the latent conversion hazard.

But such a seam does not turn the synthetic state into a reachable genuine defect. Introducing the typed outcome model, changing conversion semantics, or adding a non-test hook is correction work and belongs to BA0 or a maintainer-approved rescope.

The current BA0 instruction requiring a new Svelte-refusal target to be RED is itself an unsupported assumption ([BA0.md](docs/arch/refactor/rev11/charters/BA0.md:37)). After RT-1, that target may simply pass because genuine refusals already take the atomic `Err` arm. A passing regression is appropriate; manufacturing a RED target is not.

### 4. Authority

A track orchestrator cannot change the finding, class, owner, or gating test of this ratified row. The orchestrator is expressly not an architecture authority and cannot silently amend architecture or gates ([governance.md](docs/arch/refactor/rev11/governance.md:23)).

The maximum it may do is:

- Record the complete construction-site and reachability evidence.
- Mark item 6 `NOT-EVIDENCED` under the current ratified bytes.
- File a deviation memo with a recommended amendment.
- Add non-production tests and supplementary evidence without mislabelling them as the ratified AT-2 gate.
- Stop BF3 acceptance and downstream unlocking pending maintainer disposition.

Only the maintainer may formally rescope or amend the charter ([governance.md](docs/arch/refactor/rev11/governance.md:285)).

### 5. Is the proposed exhaustive suite plus enumeration sufficient?

Under the current ratified row: **no; it remains `NOT-EVIDENCED`.** The suite proves other reachable atomicity classes, not the asserted AT-2 defect.

After a maintainer amendment: it can satisfy BF3’s atomicity exit and make the per-defect item-6 clause vacuous for AT-2, but only if “exhaustive” is actually proven. Merely recording the fact-empty residual as `UNKNOWN` is still an open proof gap, and the charter expressly says `UNPROVEN` cannot count as exhaustion ([BF3.md](docs/arch/refactor/rev11/charters/BF3.md:45)).

Therefore one of these must happen before exhaustion is claimed:

- Demonstrate the residual path and add a real public regression, or
- Close it with conclusive source/structural proof and a discriminating control.

The structural site-3 correction may remain deferred to BA0, but it needs the typed-outcome acceptance described above.

Maintainer act, verbatim:

> **AMEND AT-2: reject the ratified claim that a reachable batch entry currently publishes a product with a genuine typed refusal; reclassify AT-2 as a latent HostBacked result-outcome construction hazard with reachability unproven, retain DEFER to BA0, replace its acceptance with a structurally typed success/stale-success/failure boundary and discriminating conversion tests, and remove the requirement that a Svelte-refusal atomicity target be RED unless an independently reproduced mixed outcome is first demonstrated.**
