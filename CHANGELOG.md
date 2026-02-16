# Changelog

All notable changes to this project will be documented in this file.

### Miscellaneous

- Improve working on benchmark (ecd5dd9)
- Add packages repo (2893ae0)

### Features

- Remove oxc_transform and remove typescript as plugin (474eeba)
- Improve code quality and handling (3fe41aa)

### Refactoring

- Improve template generation, improved vapor generation (#86) (3c73730)

### Ci

- Update workflow to contain changelog changing (bb355e5)
- Performance and benchmark (#85) (270f228)
- Fix benchmark and integration tests (fa95e72)
- Update benchmark and fix build (a2593ad)
- Remove pnpm version from benchmark (b4957f7)
- Update integration (538044d)
- More (efbb99c)
- Integration (041a48c)
- Update integration projects and update benchmark (d368d90)
- Improvements (32efbce)
- Integration (05c62fc)
- Fix (4140784)
- Fix integration (723f64c)
- Grep integration (c9dca0c)
- Another go (de80f02)
- Tarbal approach (a001016)
- Again (c0ce892)
- Only hoist verter (ca277ff)
- Add agentic workflow update-docs (#87) (d5748c2)
- Benchmark results (3a0bceb)
- Fix integration (843d0e3)
- Fix release (d97d194)

### Bug Fixes

- Fix generics (4c19877)
- Fix appending ctx when the name ends in a `.` (69bc5e2)
- Fix binding on multiline (319900b)
- Fix build (cf9b975)
- Fix script-block moving to 0 when is 0 (d5514eb)
- Fix(plugin): handle multi-line and improve block cleanup
fixes #38 (fd9b2b8)
- Handle when imports end in just spaces or new lines (d93e8e1)
- Literals should be ignored in bindings (220997c)
- Comment empty blocks (#54) (6571902)
- On slot declaretion create unique variable name (#70) (989fa05)
- Improve `component :is` and fix optional props (#73) (6503ea7)
- Invalid error when the return is `any[]` (00f2f99)
- Shallow unwrap defineExpose (d8f9ef5)
- Resolve the model better also making optional boolean default (46debe7)
- $event should always be ignored (4f12bbc)
- Prevent slot from narrowing too many times (9aeae42)
- Handle generics better (350a30b)
- Omitting passed props to rootElement (1c3fc53)
- Do not camelize props in elements (f30850a)
- Offset (875003f)

### Documentation

- Use github alerts for better formatting (#62) (a188ab2)

### Features

- Improve document handling (#8) (e2ab78e)
- Expose function to the binding and support typescript `declare` (7b2d1db)
- Improve bundle type (#55) (e774c15)
- Merge options with PublicInstance (#57) (0ec17a2)
- Support for Component type inferrence (#63) (5bf94ba)
- Support function type inference from template (#40) (2d7af6d)
- Use Comp to retrieve the actual component for a ref (#65) (2de35bc)
- Infer attributes from the root element (#68) (9608e98)
- Improve handling of broken expressions (#69) (afe14eb)
- Add DiagnosticsManager (85b4d76)
- ExtractComponents components can only be from capitalized (5d1f266)
- Improve slots resolution type (541c24a)
- Use shallow unwrap for the template binding (86b3e6d)
- Ignore class and style for fallthrough components (389d333)
- For element use Vue prop type to accurately infer the correct prop (545e3fd)
- Improve directive handling and modifiers (#81) (fbd33ba)
- Single bundle support (#82) (955140e)
- Add vue tokenizer in rust (#83) (a1b5f0e)
- Use Buffer instead of string when communicating between JS and Rust (00c83aa)
- Add native time to wasm (980b8d3)

### Miscellaneous

- Bump oxc (#43) (39b9baa)
- Update all (#53) (7f82f64)
- Added many examples (#60) (240b2f8)
- Run workflow in every PR (#67) (a86c9f6)
- Improve diagnostics speed (2553cfd)
- Add simple statistics gathering (38e8ece)
- Fix tests (e1cd934)
- Bump dependencies (6d9b6d1)
- Fix tests (9be189d)
- Fix tests (943124f)
- Netlify build headers (23b38e7)
- Format with oxfmt (62863aa)

### Refactoring

- Improve type helpers & components (#39) (1a102c2)

### Ci

- Remove netlify config and nightly play deploy (742068c)
- Change release targets (9312822)
