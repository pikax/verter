<script setup lang="ts">
import { ref } from "vue";

const msg = ref("Hello World!");

defineOptions({
  computed: {
    foo() {
      return "aaaa";
    },
  },

  mounted() {
    // @ts-expect-error not in the scope
    msg.value.toString();
  },
});
</script>

<template>
  <h1>{{ msg }}</h1>
  <input v-model="msg" />
  <h2>{{ foo }}</h2>
</template>
