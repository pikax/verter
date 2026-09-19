import Comp from "./components/Ratified.vue";

export const explicit = new Comp<string, number>({
  rows: [],
  project: (row) => row.length,
});

export const wrongEmitPayload = explicit.$emit("update:modelValue", "not-a-number");
