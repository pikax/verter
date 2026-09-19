import Comp from "./components/Ratified.vue";

export const explicit = new Comp<number>({ items: [0] });

export const wrongEmitPayload = explicit.$emit("change", "not-a-number");
