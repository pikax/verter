<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  props: {
    initialCount: { type: Number, required: true },
  },
  data() {
    return { rawCount: 0 };
  },
  computed: {
    doubled: {
      get(): number {
        return this.rawCount * 2;
      },
      set(v: number) {
        this.rawCount = v / 2;
      },
    },
  },
  methods: {
    increment() {
      this.rawCount += this.initialCount;
    },
    badAssign() {
      const x: string = this.doubled; // TS2322: number not assignable to string
      return x;
    },
  },
  mounted() {
    this.increment();
    this.doubled = 10;
  },
});
</script>
<template>
  <div>{{ doubled }}</div>
</template>
