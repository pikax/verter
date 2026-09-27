// Explicit instantiation expressions over a generated SFC constructor: the
// alias keeps the specialized construct signature (T = Row, K = "id").
import Picker from "./components/Picker.vue";

export interface Row {
  id: number;
  label: string;
}

export const RowPicker = Picker<Row, "id">;
