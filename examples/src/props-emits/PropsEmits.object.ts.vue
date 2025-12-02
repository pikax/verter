<script lang="ts">
import { defineComponent, type PropType } from "vue";

export default defineComponent({
  name: "PropsEmits",
  props: {
    name: { type: String as PropType<string>, required: true },
    count: { type: Number as PropType<number>, required: true },
    active: { type: Boolean as PropType<boolean>, default: false },
  },
  emits: {
    change: (value: string) => typeof value === "string",
    update: (id: number) => typeof id === "number",
  },
  methods: {
    handleClick() {
      this.$emit("change", this.name);
      this.$emit("update", this.count);
    },
  },
});
</script>

<template>
  <div>
    <p>Name: {{ name }}</p>
    <p>Count: {{ count }}</p>
    <p>Active: {{ active }}</p>
    <button @click="handleClick">Emit</button>
  </div>
</template>
