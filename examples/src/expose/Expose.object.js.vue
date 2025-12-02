<script>
import { ref } from "vue";

export default {
  name: "Expose",
  setup(_, { expose }) {
    const inputRef = ref(null);

    function focus() {
      inputRef.value?.focus();
    }

    function getValue() {
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
};
</script>

<template>
  <div>
    <input ref="inputRef" type="text" />
    <button @click="focus">Focus</button>
  </div>
</template>
