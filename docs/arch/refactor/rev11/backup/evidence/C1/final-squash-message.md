# C1 final squash-message draft

```text
refactor(core): establish the semantic module resolver boundary

Move the module-resolution kernel into verter_semantic behind immutable
observation snapshots and typed Complete/NeedInputs/Terminal outcomes.

Route workspace retries, ordered loads, replay, and witness publication through
the single production driver while preserving all historical resolution cases
and removing the legacy resolver surface.

Share the resolver-context implementation while retaining distinct host and
session lifecycle adapters, and keep cache, I/O, scheduling, and cross-request
ownership outside the semantic kernel.

Bind the registered AC5 split, exact retention limits, final C1 performance
dispositions, mutation/compile-fail/cache-fence evidence, and C2/C4 successor
obligations without changing their scope.
```

This is a draft for the landing agent. It does not authorize squash or landing.
