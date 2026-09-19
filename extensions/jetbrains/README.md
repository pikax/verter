# Verter for JetBrains IDEs — plugin skeleton (JBT1)

Gradle-based IntelliJ Platform plugin skeleton pinned to official **WebStorm
2026.2.3 (build 262.10968.77)**. It carries no semantic features: the sole
semantic entry for editor clients remains the native `verter-lsp` server, and
the skeleton only proves that a Verter plugin loads against the pinned build
and can be built, tested, verified and packaged reproducibly. JBT2 owns the
plugin lifecycle and engine integration.

## What is here

- `VerterHealthAction` (`Tools → Verter Health`): reports the installed plugin
  version and the running platform build. This is the JBT1 load proof and the
  entire user-visible surface.
- Real JVM tests: a booted-platform test asserting the action is registered
  under `verter.Health`, plus descriptor-consistency tests. JBT1H adds
  `RealIdeCaptureTest` (test hook only): versions, EDT application/paint and
  process-tree snapshot from the same pinned test IDE. The comparison recorder
  lives in `packages/dx-harness/jetbrains`.
- `verifyPlugin` runs the IntelliJ Plugin Verifier against the pinned WebStorm
  build (never `recommended()`, so verification never depends on whatever IDE
  builds are latest at run time), failing on every failure level.
- `buildPlugin` produces the installable distribution zip.

## Supported IDE range

Declared in `tests/jetbrains-product/JBT1/products/ide-support-range.v1.json`
and pinned by `gradle.properties`; the two must agree exactly or the gate
fails. Supported: **WebStorm, since-build `262.10968`, until-build `262.*`**.
Nothing else is claimed — not other JetBrains products, not other platform
branches.

## Running the gate

```bash
node scripts/jetbrains-gate.mjs            # from the repository root
# or explicitly:
node scripts/jetbrains-gate.mjs --java-home <jdk-21>
```

The gate runs `test verifyPlugin buildPlugin` through the pinned Gradle
wrapper and fails closed when the JDK, the pinned IntelliJ Platform SDK, the
build output or the test evidence is missing — there is no pass-with-no-tests
path. It also validates the declared range against the packaged descriptor
inside the built zip. CI runs the same gate on the `jetbrains-plugin` lane.

Requirements: a JDK 21 toolchain (`JAVA_HOME` or `--java-home`). The Gradle
wrapper (`gradlew`) pins Gradle itself; the IntelliJ Platform Gradle Plugin
resolves and caches the pinned WebStorm SDK on first use.

## Layout

- `gradle.properties` — every pin (product, version, build, range, plugin id).
- `build.gradle.kts` — IntelliJ Platform Gradle Plugin 2.19.0 wiring; refuses a
  `platformType` that is not `WS`.
- `src/main/kotlin/dev/verter/jetbrains/` — the health action.
- `src/test/kotlin/dev/verter/jetbrains/` — JVM tests.

## Status

Inert skeleton (JBT1). No activation, no language support, no LSP wiring. The
scaffolding is deleted/replaced once JBT2 owns the lifecycle; until then the
JBT0 baseline manifest records this adapter's state.
