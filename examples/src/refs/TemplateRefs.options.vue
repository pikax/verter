<script lang="ts">
import { defineComponent, useTemplateRef } from "vue";

export default defineComponent({
  setup() {
    const myRef = useTemplateRef("myRef");
    return {};
  },
});
</script>
<template>
  <div ref="myRef"></div>
</template>
