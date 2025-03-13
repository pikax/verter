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

let value = defineModel<string>('bar')

value.value

const foo = 1;

new ___VERTER___default().$props
</script>
<template>
  <span>{{ $data.foo }}</span>
  <span> {{ $props.modelValue }} </span>
  <span> {{ foo }} </span>
  <span>{{ $props}}</span>
</template>
