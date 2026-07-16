<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  name: "OptionsApi",
  props: {
    title: { type: String, required: true },
  },
  data() {
    return { clicks: 0 };
  },
  methods: {
    increment() {
      this.clicks += 1;
    },
  },
});
</script>

<template>
  <button type="button" @click="increment">{{ title }}:{{ clicks }}</button>
</template>
