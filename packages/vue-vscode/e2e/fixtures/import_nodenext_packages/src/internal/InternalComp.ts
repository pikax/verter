import { defineComponent } from "vue";

// Reached via the package `imports` map (`#internal/*` -> `./src/internal/*`).
export const InternalComp = defineComponent({
  props: {
    internalOnly: { type: String, required: true },
  },
  template: "<div>{{ internalOnly }}</div>",
});
