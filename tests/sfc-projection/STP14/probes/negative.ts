import Comp from "./components/Capture.vue";

// Lifting must not hide a real source error: the lifted alias keeps its
// authored `A["id"]` member type, so a wrong key stays a customer
// diagnostic instead of collapsing into a permissive shape.
export const wrongKey = new Comp<{ id: number }>({
  selected: { item: { id: 1 }, key: "one" },
});
