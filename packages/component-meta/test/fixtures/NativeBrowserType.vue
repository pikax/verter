<script lang="ts">
import type { PropType } from "vue";
import { defineComponent } from "vue";

export default defineComponent({
  props: {
    canvas: {
      type: Object as PropType<HTMLCanvasElement>,
      required: true,
    },
  },
});
</script>

<template><div /></template>
