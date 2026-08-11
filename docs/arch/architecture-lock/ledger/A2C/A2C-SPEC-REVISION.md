Verdict: **BLOCKING FINDINGS — replace the retained representation; keep the gate and eager construction.**

The implementation correctly stopped. The evidence digests match the manifest, and the worktree is clean at `70ea4c01bea870e9684a66f229230808aeb64235`.

The original specification over-retained D6-oriented detail for an A2C consumer that needs exactly one fact. The 48-byte statement table, target table, two `Arc` allocations, reverse suffix pass, and sort are not the minimum durable representation. Calling them “minimum” was wrong.

The ruling authorizes a new implementation attempt; it does **not** prove or pre-approve landing.

## Actionable findings

1. **[A2C-SPEC.md §1](<EVIDENCE>/A2C/A2C-SPEC.md:5): retain only the endpoint fact.**

   Keep the completion algebra and typed unknown vocabulary, but make it construction-only. Delete the retained types:

   - `CompletionTarget`
   - `StatementCompletionFact`
   - `FunctionCompletionFacts`
   - `NormalCompletionFact`
   - `NormalCompletionDisposition`

   `CompletionTargetId`, `CompletionSet`, `CompletionFact`, `CompletionTargetKind`, and `AuthoredReturnMembership` become `pub(crate)` construction types. A3’s only public fact remains:

   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   #[repr(u8)]
   pub enum CompletionUnknown {
       TargetCapacityExceeded = 0,
       UnresolvedBreakTarget = 1,
       UnresolvedContinueTarget = 2,
       LoopFlowRequired = 3,
       ConditionalFlowRequired = 4,
       FinallyInferenceFlowRequired = 5,
       UnsupportedWithStatement = 6,
       UnsupportedRecoveredSyntax = 7,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   #[repr(u8)]
   pub enum EndpointUndefinedDisposition {
       DoesNotContribute = 0,
       Contributes = 1,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   pub enum EndpointUndefinedFact {
       Exact(EndpointUndefinedDisposition),
       Unknown(CompletionUnknown),
   }
   ```

   The `CompletionSet` layout and compositional operations in existing §4 remain unchanged, but the set is never retained in a published skeleton.

2. **[flow/mod.rs `SkeletonRegion`](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:175): use existing region storage instead of growing the skeleton.**

   Replace `pub has_return: bool` with a private one-byte flag carrier:

   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr)]
   #[repr(transparent)]
   struct SkeletonRegionFlags(u8);

   impl SkeletonRegionFlags {
       const HAS_RETURN: u8 = 1 << 0;
       const ENDPOINT_SHIFT: u8 = 1;
       const ENDPOINT_MASK: u8 = 0b0001_1110;

       const ENDPOINT_DOES_NOT_CONTRIBUTE: u8 = 0;
       const ENDPOINT_CONTRIBUTES: u8 = 1;
       const ENDPOINT_UNKNOWN_BASE: u8 = 2;
   }
   ```

   Encoding in bits 1–4 is exact:

   ```text
   0       Exact(DoesNotContribute)
   1       Exact(Contributes)
   2..=9   Unknown(CompletionUnknown(code - 2))
   ```

   Bits 5–7 must remain zero. Only root region zero carries endpoint meaning. Non-root endpoint bits must remain zero and must never be read.

   Add:

   ```rust
   impl SkeletonRegion {
       #[must_use]
       pub const fn has_return(&self) -> bool;

       fn mark_has_return(&mut self);

       fn set_root_endpoint(&mut self, fact: EndpointUndefinedFact);

       fn root_endpoint(&self) -> EndpointUndefinedFact;
   }
   ```

   Update the direct readers at [flow_slice_content.rs:3033](<REPO>-wt-a2c/crates/verter_session/src/flow_slice_content.rs:3033) and existing tests from `.has_return` to `.has_return()`.

3. **[A2C-SPEC.md §2](<EVIDENCE>/A2C/A2C-SPEC.md:238): do not add a `FunctionBodySkeleton::completion` field.**

   Use exactly:

   ```rust
   impl FunctionBodySkeleton {
       #[must_use]
       pub fn endpoint_undefined_disposition(&self) -> EndpointUndefinedFact {
           self.regions[0].root_endpoint()
       }
   }
   ```

   Delete `statement_completion`. A3 has no legitimate statement-fact lookup.

   Required locked layouts on 64-bit targets:

   ```rust
   assert_eq!(std::mem::size_of::<SkeletonRegionFlags>(), 1);
   assert_eq!(std::mem::size_of::<SkeletonRegion>(), 32);
   assert_eq!(std::mem::size_of::<FunctionBodySkeleton>(), 96);
   ```

   If either existing type grows, the implementation fails. Do not compensate with another allocation.

4. **[A2C-SPEC.md §3](<EVIDENCE>/A2C/A2C-SPEC.md:279): retain no completion vectors.**

   Delete:

   ```text
   completion_statements
   completion_targets
   last_statement_completion
   body_completion
   reverse draft-index pass
   statement-fact sort
   Arc slice publication
   ```

   Construction requirements become:

   ```text
   - visit_statement_list returns CompletionDraft;
   - it composes statements forward in authored order using sequence;
   - no completed-statement draft is retained after it has been composed;
   - target state is transient and fixed-capacity;
   - completion construction performs zero heap allocations and reallocations;
   - finish encodes only endpoint_undefined into root-region flags.
   ```

   Use one fixed inline target stack:

   ```rust
   active_completion_targets: [Option<ActiveCompletionTarget<'ast>>;
                               MAX_COMPLETION_TARGETS],
   active_completion_target_len: u8,
   completion_overflowed: bool,
   ```

   Target bits are stack-slot identities and may be reused after their construct exits. Capacity means **65 simultaneously live targets**, not 65 targets appearing sequentially. The overflow test must therefore use 65 nested targets.

   Do not intern labels merely for completion routing. `ActiveCompletionTarget` borrows the retained AST label text for the duration of construction.

5. **[A2C-SPEC.md §5](<EVIDENCE>/A2C/A2C-SPEC.md:466): A3 still reads only the endpoint.**

   Retain the existing A3 logic, with no statement lookup:

   ```rust
   match skeleton.endpoint_undefined_disposition() {
       EndpointUndefinedFact::Exact(expected)
           if expected == observed_endpoint_undefined => {}
       EndpointUndefinedFact::Exact(_) | EndpointUndefinedFact::Unknown(_) => {
           // FlowGap::AbruptCompletion; suppress warm admission.
       }
   }
   ```

   This one fact has the full required discriminating power:

   - G10: expected absent, legacy observation present → retract.
   - X05: absent equals absent → clean.
   - X68/X80: present equals present → clean.
   - X88: absent equals absent → clean.

   `body`, statement completion, suffix-normal records, target spans, target kinds, and authored-return membership are unnecessary retained data for A3.

6. **[A2C-SPEC.md §6](<EVIDENCE>/A2C/A2C-SPEC.md:547): preserve non-interference.**

   Replace “facts are written into the skeleton” with:

   > The sole retained A2C fact is encoded in the existing root `SkeletonRegion` flag byte. It enters no graph, slice, stable hash, cache key, persistence format, admission decision, or public result during A2C.

   There remains no cache epoch or admission change.

7. **[A2C-SPEC.md §7](<EVIDENCE>/A2C/A2C-SPEC.md:573): the performance gate does not change.**

   Replace the retained-size clause with:

   ```text
   Retained A2C cost per function:
     FunctionBodySkeleton growth: 0 bytes
     SkeletonRegion growth: 0 bytes
     statement facts: 0 bytes
     target facts: 0 bytes
     completion-owned allocations: 0
     completion-owned reallocations: 0
   ```

   Keep the construction gate:

   ```text
   upper slowdown = max(3%, 2 × predeclared measured noise floor)
   ```

   Require at least 30 interleaved baseline/candidate samples and the stable control required by Revision 11 verification. Criterion’s comparison against an unrelated saved estimate is not gate evidence.

   The 64- and 65-live-target shapes remain latency cells. The 65 case must fail closed as `TargetCapacityExceeded`.

   **Honesty constraint:** zero bytes and zero allocations prove removal of the measured retained-payload cause, but they do not prove wall latency. No source-only argument can prove a timing gate. A new exact candidate must run the benchmark. If the compact fused implementation still exceeds 3%, stop again; do not relax the gate or move construction to first query.

8. **[A2C-SPEC.md §8](<EVIDENCE>/A2C/A2C-SPEC.md:631): revise the tests.**

   Keep the algebra, G10, X05, X68, X80, X88, mutation, determinism, linear-work, and cold/warm tests.

   Replace:

   ```text
   completion_fact_layout_is_locked
   a2c_completion_spans_are_frame_relative
   completion_target_65_is_typed_unknown_not_truncated
   ```

   with:

   ```text
   completion_transient_layout_is_locked
   a2c_completion_storage_has_zero_retained_growth
   completion_65_simultaneously_live_targets_is_typed_unknown
   completion_65_sequential_targets_reuses_slots_exactly
   a2c_completion_construction_allocates_zero
   ```

   Delete tests for statement lookup, retained target spans, statement sorting, and retained statement counts.

9. **Strongest counter-argument and stop condition.**

   Dropping statement facts means D6 cannot later obtain a ready-made per-statement table. That is intentional. [AMD-002](<REPO>-wt-a2c/docs/arch/refactor/rev11/amendments/AMD-002-a2c-completion-predecessor.md:36) requires D6 to consume the same **completion algebra**, not the rejected 48-byte storage schema. D6 may extend the sole skeleton/graph construction when it owns graph edges and state routing, but it must call this algebra and must not introduce another classifier.

   If D6 ultimately proves that a retained per-statement structural product is necessary, D6 must specify and benchmark that product against its actual graph consumer. A2C must not prepay that cost on speculation.

## Lazy-build ruling

A separate lazy completion memo is **not admissible**. It would require either:

- retaining AST/arena references inside `FunctionBodySkeleton`; or
- reopening the retained parse snapshot and walking function syntax on A3 demand.

Both violate “computed once during skeleton construction” and “no query-time AST rewalk.” A `OnceLock`, regardless of key or lifetime, would merely memoize the violation. Therefore there is no memo key, owner, or cache-admission change to specify.

## Gate ruling

The present evidence does not justify weakening the 3% gate. It measures a candidate dominated by 157 additional allocations on the 64-target shape and a 10,616-byte payload that A3 never consumes. That is not evidence that the minimum eager classifier inherently exceeds the gate.

If a zero-growth, zero-allocation implementation later fails, that becomes evidence of a materially false performance premise and requires the formal blind recalibration/restart procedure. It cannot be resolved from this already-observed candidate.

## Block viability and ownership

A2C remains viable, but is still **BLOCKED pending a fresh candidate and green performance evidence**.

The boundary is unchanged:

- D5 owns effects, captures, freshness, and escape.
- D6 owns graph edges, state routing, loops, fixed points, and any graph-required structural payload.
- D8 owns proof-carrying completion and warm admission.
- A2C owns the completion vocabulary, composition rules, and the single endpoint fact A3 consumes.

No working-tree change should be made to the clean base until this replacement specification is adopted as the new implementation authority.

__DONE__
