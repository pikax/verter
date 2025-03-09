<script setup lang="ts">
import { defineOptions } from "vue";
defineOptions({
  data() {
    return { foo: "hello" };
  },
});

const foo = 1;
</script>
<template>
  <span>{{ $data.foo }}</span>
  <span> {{ foo }} </span>
</template>
