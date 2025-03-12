<script setup lang="ts">
import { defineOptions } from "vue";
defineOptions({
  
  data() {
    return { foo: "hello" };
  },
});

defineProps({
  bar: String,
});

let value = defineModel('bar')

const foo = 1;

// const $props = {jjj: true}
</script>
<template>
  <span>{{ $data.foo }}</span>
  <span> {{ $props.modelValue }} </span>
  <span> {{ foo }} </span>
</template>
