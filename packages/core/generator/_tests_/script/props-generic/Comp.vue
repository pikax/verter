<script lang="ts" generic="T extends string">
import { PropType } from "vue";

export default {
  props: {
    foo: {
      type: String as PropType<T>,
      required: true,
    },
  },
};
</script>
<template>
  <span>1</span>
</template>
