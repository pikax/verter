# BS0 — Immediate Svelte correction

**Status:** PROPOSED / **RATIFIED (AMD-009)**; not accepted. **Class:** Framework subsystem.
**Predecessor:** BF3. **Downstream:** acceptance is a mandatory predecessor of B2
and B3.

## Objective

Correct the four ratified Svelte findings from BF3 in their reusable root-cause
owners before B2/B3. BS0 does not wait for the post-B4 BS1 conformance train and
does not add a production retraction path.

## Owned scope

BS0 owns exactly:

1. **SV-1:** Svelte client block planning in `client_block_plan` for keyed runes
   `{#each}` flags.
2. **SV-2:** Svelte client semantic lowering for accepted instance-script `$props()`
   reads.
3. **SV-3:** the Svelte runtime map builder's authored script-declaration
   provenance.
4. **SV-4:** as a distinct item, the session-side Svelte PublicApi projector's
   untyped `$props()` surface.

| finding | acceptance ID | existing discriminating test |
|---|---|---|
| SV-1 | `BF3-SV-1-EACH-FLAGS` → `FC-SVELTE-001` | `each_flags_for_a_keyed_runes_each_match_the_official_compiler` (`#[ignore]`d target); characterization `each_flags_for_a_keyed_runes_each_currently_add_the_item_reactive_bit` |
| SV-2 | `BF3-SV-2-PROPS-INSTANCE` → `FC-SVELTE-001` | `a_runes_props_read_in_the_instance_script_compiles_to_a_runtime_module` (`#[ignore]`d target); characterization `a_runes_props_read_in_the_instance_script_is_currently_refused_with_its_typed_code` |
| SV-3 | `BF3-SV-3-CLIENT-MAP-SCRIPT` → `FC-SVELTE-001` | `the_client_source_map_covers_every_required_authored_anchor` (`#[ignore]`d target); characterization `the_client_source_map_currently_carries_only_these_authored_coordinates` |
| SV-4 | `BF3-SV-4-PROPS-SURFACE` → `FC-TS-001` | `an_untyped_svelte_props_destructure_publishes_a_props_surface_typescript_sees_as_empty` (green characterization; there is no ignored target because the projector defines the correct surface) |

## Required procedure

For each item, make the public-boundary correct-behavior assertion fail first,
including a positive checker assertion for SV-4; prove the RED result is caused by
the named defect; implement the minimum reusable correction in the named owner; and
rerun the target, characterization, affected Svelte cells, and an unplanted control.
The correction must preserve request identity and product atomicity and must not be
implemented as refusal, withholding, tracking, or fixture recognition.

## Required exits

All four acceptance IDs pass their named discriminating boundary tests. The three
ignored correct-behavior targets are enabled and green. SV-4 publishes the
projector-defined correct TypeScript surface under the pinned Svelte declaration
closure, with its former empty-surface characterization converted into a permanent
discriminator. Applicable `FC-SVELTE-001`, `FC-TS-001`, route, mapping, and
atomic-publication checks stay non-vacuous.

The exact corrected cells pass on every public route that reaches the shared owner,
and unrelated supported Svelte cells retain behavior. Only BS0 acceptance satisfies
this block's B2/B3 predecessor edge.

## What it must NOT do

BS0 must not implement BA0, BCSS0, BRT0, B3, B4, or BS1 work; add production
retraction, defect-selected typed refusal, artifact withholding, fixture-identity
branches, or a version-specific divergence list; infer semantics by scanning source
or generated strings as a second authority; or absorb the stale `svelte@5.56.3` pin
and corpus migration, which is expressly excluded from this package.

## Abort/rescope

Stop with `RESCOPE_REQUIRED` if an owned defect cannot be corrected in the named
root-cause owner without taking B3/B4 authority or changing a ratified product
contract. Route the required repair to the appropriate ratified correction scope;
never substitute a retraction path or fold another immediate owner's work into BS0.
