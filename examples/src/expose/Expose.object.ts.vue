<script lang="ts">
import { defineComponent, ref, type Ref } from "vue";

export default defineComponent({
  name: "Expose",
  setup(_, { expose }) {
    const inputRef: Ref<HTMLInputElement | null> = ref(null);

    function focus(): void {
      inputRef.value?.focus();
    }

    function getValue(): string {
      return inputRef.value?.value ?? "";
    }

    expose({
      focus,
      getValue,
    });

    return {
      inputRef,
      focus,
    };
  },
});
</script>

<template>
  <div>
    <input ref="inputRef" type="text" />
    <button @click="focus">Focus</button>
  </div>
</template>
