<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  props: {
    msg: { type: String, required: true },
  },
});
</script>
<template>
  <div>{{ msg }}</div>
</template>
