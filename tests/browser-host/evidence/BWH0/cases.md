# BWH0 evidence index

Selected case IDs: `BWH0-AC1`, `BWH0-AC2`, `BWH0-AC3`, `BWH0-ratification`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node tests/browser-host/BWH0/verify.mjs`
- `node --test tests/browser-host/BWH0/bwh0.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
- the remaining `docs-domain` final profile commands (see `catalogs/gate-profiles.toml`)

Deletion population this node: empty. No route is displaced or retired by BWH0; the playground keeps consuming `@verter/wasm` directly until the shared browser host exists.

BWH0.1 grounding: the dependency-closure census ran `cargo tree --target wasm32-unknown-unknown -p verter_wasm -e normal` on the ratification candidate (181 crates; count recorded, not pinned). The census found no tokio/rayon/mio/nix/tempfile/socket2 edge reachable from the wasm32 build; `windows-sys`/`windows-link` resolve only through the inert `ts-rs → termcolor → winapi-util` chain. The platform seams (web_time Instant, getrandom wasm_js + uuid js, scheduler sync inline drive with the driver thread/rayon batch compiled out, ambient-library reads native-only) and the playground's fail-closed engine pins (`capabilityForWasm` / `tsgo: false`, `typescript@6.0.3`, no SharedArrayBuffer or crossOriginIsolation usage) were read from the shipped sources; the harness re-reads them live so drift fails rather than stales.

AC-BASIS is bound to BWH1 (recurring build proof) and BWH8 (browser cache/artifact integrity) as downstream runtime-test owners; AC-RESOURCE is not applicable (no hot paths, no UI, 0 production LOC) and binds to BWH11/VSW5 for future real-client claims; AC-EXPOSURE registration is owned by DX1.

This file does not claim the cases executed. Copying the charter here is not evidence.
