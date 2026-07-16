/**
 * Dense Svelte IDE feature matrix (markup, events, module, JS, runes, CSS).
 */
import { SVELTE_MATRIX_CASES } from "../../../lib/matrixCases";
import { registerMatrixSuite } from "../../../lib/registerMatrix";

registerMatrixSuite({
  title: `Svelte feature matrix`,
  fixture: "svelte-parity",
  entry: "src/App.svelte",
  cases: SVELTE_MATRIX_CASES,
});
