/**
 * Dense Vue IDE feature matrix (directives, slots/emits, style bind, JS, mapping negatives).
 */
import { VUE_MATRIX_CASES } from "../../../lib/matrixCases";
import { registerMatrixSuite } from "../../../lib/registerMatrix";

registerMatrixSuite({
  title: `Vue feature matrix`,
  fixture: "vue-parity",
  // App imports the whole parity corpus. Certify the first case locally;
  // registerMatrixSuite separately awaits the complete workspace frontier.
  entry: "src/matrix/Directives.vue",
  cases: VUE_MATRIX_CASES,
});
