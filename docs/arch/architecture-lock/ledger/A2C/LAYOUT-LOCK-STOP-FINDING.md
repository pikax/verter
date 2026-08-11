# A2C layout-lock stop finding

STATE: BLOCKED

The locked base `70ea4c01bea870e9684a66f229230808aeb64235`
has these measured 64-bit layouts on the required execution target:

```text
FunctionBodySkeleton: 96 bytes
SkeletonRegion: 28 bytes
```

`A2C-SPEC-REVISION.md` simultaneously requires:

```text
SkeletonRegion growth: 0 bytes
size_of::<SkeletonRegion>() == 32
If either existing type grows, the implementation fails.
```

A 32-byte `SkeletonRegion` is a four-byte increase from the locked base, so
the three mandates cannot be satisfied by one candidate. The exact compact
encoding reaches the behavioral test surface, but the mandatory storage test
correctly fails with `left: 28`, `right: 32`.

No candidate commit exists. The construction-performance comparison and full
gate are not valid to run without a layout-conforming candidate. The A2C
worktree is restored to the clean locked base.

Raw proof: `command-proofs/layout-lock-contradiction.txt`.
