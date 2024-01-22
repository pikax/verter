<script lang="ts">
import { PropType } from "vue";

defineProps<{ foo: string }>();

export default {
  props: {
    foo: {
      type: String as PropType<"foo">,
      required: true,
    },
  },
};
</script>
<template>
  <span>1</span>
</template>
