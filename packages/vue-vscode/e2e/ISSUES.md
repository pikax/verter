# E2E parity issues

Open gaps found while plumbing VS Code / LSP parity coverage for Vue and Svelte.
Every `ISSUE-*` referenced from E2E sources (except unit tests) MUST appear as a table row.

Status: `open` · `partial` · `fixed`

| ID | Area | Status | Symptom | Expected | Notes |
|---|---|---|---|---|---|
| ISSUE-4 | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-code-action-apply-organize | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-code-action-on-errors | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-depth-mapping-event | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-depth-mapping-slot | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-depth-rename-apply | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-depth-undo-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-dom-event-over-inference-boundary | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-hash-imports | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-kit-routes | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-nuxt-composable | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-nuxt-pages | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-svelte-lib | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-svelte-lib-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-svelte-lib-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-vue-alias | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-eco-vue-alias-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-editor-call-hierarchy | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-editor-document-links | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-editor-folding | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-editor-selection-range | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-find-def-refs-consistency | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-find-function | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-find-js-exact | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-find-ts-exact | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-dom-event-config | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-dom-event-jsdoc | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-dom-event-non-inference | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-hover-markup | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-lax-dom-event-config | triage | fixed | js-lax carriers bind through the discovered jsconfig project: `shared.js-lax.dom-event.unannotated-remains-any` and `shared.js-lax.dom-event.diagnostics-follow-config` executed green on vue-parity@tsserver and vue-parity@tsgo; neutral-LSP jsconfig carrier rows green on all three provider routes | jsconfig discovery + admission | Hard-fail suite (no catch-all skip) |
| ISSUE-js-lax-dom-event-non-inference | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-references | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-rename-markup | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-js-rename-script | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-lifecycle-external-ts | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-lsp-document-format | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-lsp-document-highlights | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-lsp-semantic-tokens | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-lsp-signature-help | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-lsp-type-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mapping-highlights | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mapping-hover-range-member | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mapping-hover-range-script | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mapping-hover-range-template | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mixed-child-prop-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mixed-cross-import | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mixed-svelte-child | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mixed-svelte-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mixed-vue-entry | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mixed-vue-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-mixed-wrong-prop-types | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-multi-root-dual-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-multi-root-folders | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-multi-root-isolation | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-multi-root-svelte-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-multi-root-vue-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-perf-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-perf-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-product-emmet | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-product-extract-component | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-product-inlay-hints | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-product-organize-imports | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-rename-js-function | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-rename-reject-html | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-rename-ts-markup | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-shared-document-symbols | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-shared-rename-apply | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-shared-rename-from-markup | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-shared-rename-prepare | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-shared-workspace-symbols | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-class-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-class-definition-template | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-id-references | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-svelte-global | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-svelte-global-local | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-vue-global-local | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-style-vue-scoped | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-await | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-bad-prop-diagnostic | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-bind-value | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-bind-value-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-bindable | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-button-type-literal | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-component-tag-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-confidence-cross-file | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-confidence-hover-any | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-confidence-invalidation | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-confidence-multi-file | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-confidence-neg-battery | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-confidence-no-virtual | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-confidence-revert | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-derived-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-dom-event-current-target | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-each-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-effect | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-effect-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-default | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-event-infer | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-expect-error | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-host | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-hover-event | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-hover-field | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-hover-prop-num | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-hover-prop-str | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-hover-slot-num | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-hover-slot-str | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-infer-bad | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-infer-good | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-multi-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-prop-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-generic-slot-wrong | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-directive | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-event-attr | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-event-handler | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-local | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-narrow | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-props | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-slot-name | triage | fixed | `ide.complete.slot-or-snippet-names` executed green on svelte-parity@tsserver and svelte-parity@tsgo (child snippet-prop names) | Snippet-slot completion | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-complete-slot-prop-member | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-def-event | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-def-event-name | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-def-prop-attr | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-def-slot-name | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-def-slot-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-hover-directive-doc | triage | partial | directive keyword docs + transition-family function hovers (shim-free): GREEN on the LSP-native contract and on svelte-parity@tsgo (`ide.hover.directive-doc` executed); RED on the extension @tsserver route | Editor-route directive docs | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-hover-slot-name | triage | partial | snippet-name/render-callsite hover: GREEN on the LSP-native contract and on svelte-parity@tsgo (`ide.hover.slot-name` executed); RED on the extension @tsserver route (editor defers carrier hovers to the plugin) | Editor-route snippet hover | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-ide-hover-slot-prop-pattern | triage | partial | snippet-parameter hover: GREEN on the LSP-native contract and on svelte-parity@tsgo (`ide.hover.slot-prop-pattern` executed); RED on the extension @tsserver route | Editor-route parameter hover via plugin | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-if-narrowing | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-if-narrowing-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-if-narrowing-intellisense | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-intrinsic-attr-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-intrinsic-element-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-intrinsic-elements-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-intrinsic-type-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-js-daily | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-js-wrong-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-jsx-intrinsics | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-markup-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-markup-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-markup-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-await-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-bind-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-bindable-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-class-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-directives-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-each-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-effect-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-events-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-events-handler | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-events-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-events-tag | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-generic-infer-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-html | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-ide-surface-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-if-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-js-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-js-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-js-daily-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-js-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-js-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-js-hover-range | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-js-refs | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-mapping-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-module-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-module-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-narrowing-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-no-virtual | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-onclick | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-runes-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-scoped-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-snippet-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-snippet-correct-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-strict-rest-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-matrix-style-color | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-prop-attr-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-prop-attr-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-prop-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-props-destructure-hover | triage | fixed | `svelte.runes.props-destructure-hover` executed green on svelte-parity@tsserver | Props destructure hover | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-public-surface-empty | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-public-surface-leak | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-slots-correct-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-slots-expect-error | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-slots-local-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-slots-local-hover | triage | fixed | `slots.local.hover-typed` executed green on svelte-parity@tsserver and svelte-parity@tsgo | Snippet local hover | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-slots-wrong-props | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-slots-wrong-render | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-snippet | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-state-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-state-interpolation-diagnostic | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-strict-rest-props | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-strict-unknown-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-testing-api-isolation | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-testing-api-no-virtual | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-testing-api-setting | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-type-neg-directives | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-type-neg-events | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-type-neg-expect-error | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-svelte-type-neg-props | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-ts-code-actions | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-ts-dom-event-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-ts-dom-event-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-ts-dom-event-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-ts-dom-event-invalid-member | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-ts-rename-script | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-definition-after-edit | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-hover-after-edit | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-js-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-member-after-select | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-member-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-rapid-edit | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-recovery | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-tag-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-typing-undo | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-bad-prop-diagnostic | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-component-tag-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-cross-file | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-generic | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-hover-any | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-invalidation | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-multi-file | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-neg-battery | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-no-virtual | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-confidence-revert | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-deep-fallthrough-aria | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-deep-fallthrough-class | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-deep-fallthrough-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-deep-fallthrough-data | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-deep-fallthrough-listener | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-deep-fallthrough-style | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-define-expose | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-defineEmits-event-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-defineModel-navigation | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-defineProps-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-defineSlots-locals | triage | fixed | `vue.macro.defineSlots.slot-local` executed green on vue-parity@tsserver and vue-parity@tsgo | defineSlots locals | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-dual-script | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-dynamic-bind | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-fragment-fallthrough | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-default | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-event-infer | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-expect-error | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-host | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-hover-event | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-hover-field | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-hover-prop-num | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-hover-prop-str | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-hover-slot-num | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-hover-slot-str | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-infer-bad | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-infer-good | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-multi-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-prop-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-sfc | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-generic-slot-wrong | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-global-custom-directive | triage | open | Globally registered custom directives (`app.directive('my-thing', …)` in an entry file) fail closed for hover/definition — analysis is strictly per-file and no typed infrastructure observes runtime `app.*` registrations; component-local setup bindings/imports resolve (D6) | Typed hover + navigation for app-level registrations | Deferred with a debt row (closure-backlog D11): needs entry-file discovery + `app.*` call extraction into typed IR + a workspace registry with invalidation |
| ISSUE-vue-ide-complete-directive | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-complete-event-attr | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-complete-event-handler | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-complete-narrow | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-complete-props | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-complete-slot-name | triage | fixed | `ide.complete.slot-or-snippet-names` executed green on vue-parity@tsserver and vue-parity@tsgo (server-owned completion from the child defineSlots surface) | Slot-name completion | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-complete-slot-prop-member | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-event | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-event-name | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-kebab-event | triage | open | kebab `@my-event` → camel `myEvent` emit: GREEN on the LSP-native contract path (Rust contract tests), RED on the editor-tsserver route (`ide.def.kebab-event-to-camel-emit` executed on vue-parity@tsserver: definition not ready — the route defers definition to the TS plugin, whose component event-name surface does not resolve; same open cluster as ISSUE-vue-ide-def-event-name) | Editor-route event-name definition resolves component emits (camel and kebab) | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-kebab-prop | triage | fixed | kebab `:my-prop` → camel `myProp` declare; `ide.def.kebab-prop-to-camel-declare` executed green on vue-parity@tsserver | Case-map attr contract | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-kebab-slot | triage | open | kebab `#my-slot` → camel `mySlot` declare: GREEN on the LSP-native contract path (Rust contract tests), RED on the editor-tsserver route (`ide.def.kebab-slot-to-camel-declare` executed on vue-parity@tsserver: definition not ready; same open cluster as ISSUE-vue-ide-def-slot-name) | Editor-route slot-name definition resolves component slots (camel and kebab) | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-prop-attr | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-slot-name | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-def-slot-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-hover-custom-directive | triage | partial | custom directive typed hover + navigation to the authored declaration: GREEN on the LSP-native contract (incl. the kebab-dash caret) and on vue-parity@tsgo (`ide.hover.custom-directive` executed); RED on the extension @tsserver route | Editor-route custom-directive hover | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-hover-directive-doc | triage | partial | built-in directive doc hovers: GREEN on the LSP-native contract and on vue-parity@tsgo (`ide.hover.directive-doc` executed); RED on the extension @tsserver route (Verter-owned doc surface; the plugin cannot describe erased directive names) | Editor-route directive docs | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-hover-slot-name | triage | partial | slot-name token hover from the child defineSlots surface: GREEN on the LSP-native contract (Rust contract tests + raw-LSP probes) and on vue-parity@tsgo (`ide.hover.slot-name` executed); RED on the extension @tsserver route — the editor defers carrier hovers to the TS plugin, which has no slot-signature surface (same architectural family as ISSUE-vue-ide-def-slot-name) | Editor-route slot-signature hover | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-ide-hover-slot-prop-pattern | triage | partial | slot-props destructure pattern hover: GREEN on the LSP-native contract and on vue-parity@tsgo (`ide.hover.slot-prop-pattern` executed); RED on the extension @tsserver route — the plugin quickinfo does not yet answer at the mapped pattern position | Editor-route pattern hover via plugin | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-intrinsic-attr-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-intrinsic-element-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-intrinsic-elements-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-intrinsic-type-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-js-daily | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-js-wrong-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-markup-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-markup-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-markup-unresolved | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-click-modifier | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-completion-locals | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-computed-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-directives-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-emit-handler | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-emit-pick | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-ft-aria | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-generic-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-generic-infer-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-ide-surface-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-js-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-js-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-js-daily-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-js-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-js-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-js-hover-range | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-js-refs | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-macros-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-mapping-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-narrowing-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-no-virtual | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-no-virtual-tag | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-scoped-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-slot-body | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-slot-correct-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-slot-header | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-slots-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-strict-ft-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-style-bind-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-style-bind-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-style-bind-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-teleport | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-vbind-class | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-vfor-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-vhtml-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-vif-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-vmodel-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-matrix-von-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-native-vmodel-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-native-vmodel-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-no-inherit-attrs | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-options-api | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-prop-attr-definition | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-public-surface-empty | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-public-surface-leak | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-slots-correct-clean | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-slots-expect-error | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-slots-local-def | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-slots-local-hover | triage | fixed | `slots.local.hover-typed` executed green on vue-parity@tsserver and vue-parity@tsgo; pattern positions hover typed via the mapped slot IIFE destructure | Slot local hover | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-slots-wrong-names | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-slots-wrong-props | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-strict-fallthrough-accept | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-strict-unknown-prop | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-symbol-auto-import | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-testing-api-diagnostic-split | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-testing-api-public-isolation | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-testing-api-setting | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-testing-api-spec-bindings | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-type-neg-directives | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-type-neg-events | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-type-neg-expect-error | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-type-neg-props | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-vfor-hover | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-vif-narrowing | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-vif-narrowing-completion | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-vif-narrowing-intellisense | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |
| ISSUE-vue-withDefaults | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |

## Hardening rules

1. **No catch-all skip**: `failParityGap` throws `PRODUCT_GAP` / `TEST_DEFECT` — never `context.skip()`.
2. **Matrix hard-fails**: every accepted matrix ID is release-required.
3. **Fixture-scoped discovery**: specialty fixtures only load matching suite globs (`fixtureSuiteMap.ts`).
4. **Failure detail**: run summary includes `failedTests[]` with message + stack.
5. **Ledger completeness**: `issueLedger.unit.test.ts` fails if any ISSUE-* is missing from this file.
6. **Svelte clean diagnostics**: do **not** mask TS7026 with permissive ambient JSX in the required clean gate (see ISSUE-svelte-jsx-intrinsics).
7. **Public vs testing surface**: non-test imports must not expose script-setup internals (negative public-type tests).

## Regenerating

```bash
node packages/vue-vscode/e2e/scripts/gen-issues-ledger.mjs
```
