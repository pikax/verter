<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  props: {
    label: { type: String, required: true },
  },
  data() {
    return { message: "hello" };
  },
  methods: {
    badMethod() {
      const x: number = this.message; // TS2322: string not assignable to number
      return x;
    },
  },
});
</script>
<template>
  <div>{{ label }}</div>
</template>
