# Curated Web Platform Tests syntax inputs

These fixtures are derived test inputs from
[`web-platform-tests/wpt`](https://github.com/web-platform-tests/wpt) at
revision `7aed6630812b20e6eec2a2e40594f8dfda036e00`, verified on
2026-07-30. They are distributed under the upstream 3-Clause BSD license in
`LICENSE.md`.

The source cases come from `css/css-syntax/`,
`css/selectors/parsing/`, `css/selectors/selectors-namespace-001.xml`,
the named `css/selectors/nth-*-of-complex-selector.html` tests, and
`css/css-nesting/parsing.html`. `MANIFEST.tsv` maps every committed `.css`
file to its upstream path and case. Its final column records independently
derived semantic expectations: exact token kinds, flags, and spans; typed
recovery diagnostics, recovery spans, and rule resumption; or exact selector
facts and spans. The fixture runner compares those facts as well as checking
manifest-to-directory parity.

WPT browser tests commonly embed CSS in `<style>` elements or JavaScript
string literals. The derived files extract those authored CSS payloads. When
several payloads are needed, the manifest records the exact joining or wrapper
bytes. No network access or sibling WPT checkout is used by the test suite.
