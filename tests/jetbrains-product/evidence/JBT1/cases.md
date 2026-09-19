# JBT1 evidence index

Selected case IDs: `JBT1-AC1`, `JBT1-AC2`, `JBT1-AC3`, `JBT1-AC-OWNER`,
`JBT1-AC-BASIS`, `JBT1-AC-RESOURCE`, `JBT1-AC-EXPOSURE`.

Commands (run on the candidate; do not treat this file as an execution
transcript):

- `node --test scripts/jetbrains-gate.test.mjs` — every fail-closed
  discrimination of the gate without a JVM (19 cases: missing JDK, failed
  Gradle run, pass-with-no-tests, failing JVM tests, missing distribution,
  corrupt/unpackable or hostile distribution zip, descriptor drift, missing
  verifier reports, live pin agreement).
- `node scripts/jetbrains-gate.mjs --java-home <jdk-21>` — the real
  jetbrains-product gate: Gradle `test verifyPlugin buildPlugin` on the pinned
  WebStorm 2026.2.3 SDK through the wrapper, then post-build evidence checks.
- `extensions/jetbrains/gradlew test` — the JVM tests themselves (booted
  platform test IDE from the pinned SDK).
- CI runs the same gate on the `jetbrains-plugin` lane
  (`.github/workflows/ci.yml`), filtered on `extensions/jetbrains/**` and
  `scripts/jetbrains-gate*.mjs`.

JBT1.1 grounding: the skeleton loads against the pinned SDK with exactly one
user-visible surface (the health action under `verter.Health`, registered in
plugin.xml and asserted live against the booted ActionManager in the JVM
tests). No feature work, no semantic engine: plugin.xml depends on
`com.intellij.modules.platform` only, asserted by a descriptor test, and the
JBT0 baseline manifest's adapterState row for `extensions/jetbrains` moves
from `absent` to `skeleton-inert` with this node.

JBT1.2 grounding: the gate fails on every missing-input shape (AC1 twins) and
cannot pass with zero tests or zero build output. The `jetbrains-product`
gate profile starts at this script; CI wires it as its own lane with the JDK
21 toolchain.

JBT1.3 grounding: the supported IDE edition/build range is declared exactly
once in products/ide-support-range.v1.json and pinned once in
extensions/jetbrains/gradle.properties; the gate and the Gradle build both
refuse drift between them, and the packaged descriptor must carry the same
range. Unsupported editions are named as not claimed. Comparator capture pins
stay JBT1H pending-capture slots.

AC-OWNER: one final owner (`expansion.jetbrains-product`); retirement
obligation — the skeleton scaffolding is deleted once JBT2 owns the lifecycle
(charter migration rule).

AC-BASIS/AC-RESOURCE/AC-EXPOSURE: no caching/mapping/snapshot/result-handle
surface is changed, no hot path or UI surface beyond the health action is
touched, and no supported semantic operation is exposed; the corresponding
runtime/registration obligations bind to JBT1H/JBT10, the later train nodes
and DX1 respectively, and are not fabricated here.

Deletion population this node: empty. No existing route is displaced or
retired by JBT1 (the JBT0 products' adapterState row is a state update, not a
deletion).

This file does not claim the cases executed. Copying the charter here is not
evidence.
