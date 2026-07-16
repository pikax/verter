/**
 * Dense Vue IDE feature matrix (directives, slots/emits, style bind, JS, mapping negatives).
 */
import { VUE_MATRIX_CASES } from "../../../lib/matrixCases";
import { registerMatrixSuite } from "../../../lib/registerMatrix";

registerMatrixSuite({
  title: `Vue feature matrix`,
  fixture: "vue-parity",
  entry: "src/App.vue",
  cases: VUE_MATRIX_CASES,
});
