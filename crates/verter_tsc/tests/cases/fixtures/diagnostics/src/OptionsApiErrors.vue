<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  props: {
    label: { type: String, required: true },
    count: { type: Number, default: 0 },
  },
  data() {
    return { message: "hello", items: [1, 2, 3] };
  },
  computed: {
    doubled(): number {
      return this.count * 2;
    },
    uppercased(): string {
      return this.message.toUpperCase();
    },
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
  <div>{{ label }} {{ doubled }} {{ uppercased }}</div>
</template>
