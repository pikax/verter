# JavaScript projection evidence index

Candidate source: current STP12 worktree. Engine pins: `typescript@6.0.3`
(`ts-js`) and `typescript@7.0.2` (`ts-native`); Vue pin:
`vue@3.6.0-rc.5`.

Selected case IDs: `STP12-checkjs-off`, `STP12-checkjs-on`,
`STP12-jsdoc-generic`, `STP12-jsx`, `STP12-suppression`.

Commands and outcomes:

- `cargo test -p verter_compiler --lib vue_compiler_js -- --test-threads=1` — pass; compiles the five SFC fixtures and checks authored directives, template projection, JSDoc, JSX, and suppression behavior.
- `cargo test -p verter_compiler --lib jsx_mode_instance_declaration_uses_public_constructor_bridge -- --test-threads=1` — pass; checks the `.verter.js` public-constructor bridge.
- `node --test tests/sfc-projection/STP12/projection.spec.mjs` — pass.
- `node scripts/sfc-projection/verify-node.mjs --node STP12 --engine all --require-all --json` — qualified; both engines observe every case exactly once, including checked script/template diagnostics and the public `InstanceType` bridge.
- `cargo fmt --all -- --check` — pass.
- `cargo clippy --workspace --all-targets -- -D warnings` — pass.

Changed symbols: `instance_declaration`, the JavaScript Vue bridge fixtures,
and the STP12 protocol case runner. Deletion population: empty.

The unchecked-script fixture keeps an authored `@ts-nocheck` error while its
template control reports TS2322. The checked-script fixture reports TS2322 in
both script and template controls. The suppression reject fixture reports its
customer TS2322 error, while its `@ts-nocheck` twin proves the diagnostic would
otherwise disappear. The JSDoc fixture and its public declaration probe retain
the generic and `InstanceType<typeof Comp>` contracts; the JSX fixture preserves
authored JSX under Vue's JSX configuration.

Repair findings addressed: F1 aligns the JSX test with the shared `.verter.js`
specifier; F2 selects that test in the protocol; F3/F4 add this record; F5
binds each mandatory case to executable JS/JSX probes and compiler-read Vue
fixtures. Incremental/cancelled publication and hot-path ownership are
untouched by this projection-only boundary.
