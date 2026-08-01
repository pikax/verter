# Real-provider harness: template positions are still located by byte search

Status: open. The `verter_lsp` real-provider harness has two position locators
(`crates/verter_lsp/src/test_harness/locate.rs`), and only one of them resolves a
position through the parse.

`RealProviderTestSession::find_position` / `find_nth_position` search for a
contiguous byte string. `RealProviderTestSession::find_template_tag_position`
names a template construct — a tag name plus one valued `(attribute, value)`
pair — and resolves it through the real Vue SFC parse, so how the authored tag
wraps across lines is irrelevant.

## Where the harness stands

The byte-string locators are live and dominant: the harness holds ~160
byte-locator calls against a single `find_template_tag_position` call. An audit
of those byte-locator sites classified 117 as TEMPLATE-SEMANTIC — positions
inside a construct the parser already knows exactly, which are byte-scanned
anyway. Those 117 are open work.

## Why moving them is not mechanical

`find_template_tag_position` answers exactly one question — a position inside a
TAG NAME, disambiguated by one VALUED `(attribute, value)` pair — and that
question covers well under a third of the 117. Serving the rest needs API the
locate module does not have:

- attribute and directive NAME positions;
- positions into an attribute value or an expression;
- positions inside an interpolation;
- Svelte `{expr}` and `bind:` forms;
- slot names;
- `v-for` binding parts;
- selectors for VALUE-LESS attributes;
- structural nth-disambiguation;
- a Svelte structural path at all — the parse driven by the locate module is
  Vue-SFC only.

Until that API exists, a test whose needle would span a reflowed (multi-line) tag
is the case the structural locator serves; reflowing the fixture to suit the byte
search instead would delete the multi-line coverage that fixture exists to
provide.

## Related

- `crates/verter_lsp/src/test_harness/locate.rs` — both locators and their
  contracts.
- [`global-components-typing-and-fail-closed-diagnostics.md`](global-components-typing-and-fail-closed-diagnostics.md)
  — the deferral whose shared assertion helper is the current single caller of
  the structural locator.
