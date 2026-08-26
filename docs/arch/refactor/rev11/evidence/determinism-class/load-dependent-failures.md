# Load-dependent failures outside the closed determinism bound

**Status:** unowned. The determinism block landed and closed; its criterion was BOUNDED, not
exhaustive, and its worktree and branch are gone. These two instances fall outside that bound
and no live owner holds them.

**Why this file exists:** two instances of one class is no longer a carry. Filed as "flaky,
passes on rerun" both would be rediscovered from scratch. The diagnosis is the asset — preserve
it for whoever takes the class.

---

## Instance 2 — `verter_scheduler` (diagnosed, full detail)

`scoped_cache_publishes_before_cross_pool_installer_returns`

Observed once during the J1 base gate at `749873cb6`; the same tree re-ran fully green.

**Not caused by the branch under test.** Zero commits from that branch touch `verter_scheduler`
(control: the same query returns 1 for `verter_css_syntax`). Passes isolated, and 3/3 at crate
scope with 8 threads.

**The two panics are ONE event**, read from source rather than inferred from the failure text:
under load the leader had not published, so the joiner EXECUTED instead of deduplicating, and a
5s `recv_timeout` subsequently expired.

**Reading:** a scope-selection gap in the determinism rails, not a defect in the code under
test and not a flake. It is verbatim the determinism thesis the closed block established,
presenting outside the bound that block's criterion covered.

---

## Instance 1 — `verter_workspace` (named, diagnosis NOT captured here)

`filesystem::tests::concurrent_resolutions_are_not_refused_for_retry_exhaustion`

Recorded as the first unowned load-dependent failure outside the bound. Its diagnosis was not
captured at the time and is deliberately NOT reconstructed here — an invented diagnosis would
be worse than an absent one, and the whole point of this file is that the diagnosis is the
asset.

Whoever takes the class should re-derive it under load rather than trusting a summary. Known
context: `docs/arch/last/resolution-currency-cutover-errata.md:1387` references the test.

---

## For the owner who takes this class

The bound is the finding. Both instances are load-dependent, both are outside a criterion that
was bounded rather than exhaustive, and both pass on rerun — which is exactly the signature that
gets a real scope gap filed as noise. Scope selection in the rails is the hypothesis to test
first, because it already explains instance 2 completely.
