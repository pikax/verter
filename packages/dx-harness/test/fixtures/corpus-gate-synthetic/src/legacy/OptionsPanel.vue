<template>
  <div>
    <p>{{ summary }}</p>
    <button @click="refresh">Refresh</button>
  </div>
</template>

<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  name: "OptionsPanel",
  data() {
    return { count: 0 };
  },
  computed: {
    summary(): string {
      return "count " + String(this.count);
    },
  },
  methods: {
    refresh(): void {
      this.count += 1;
    },
  },
});
</script>
