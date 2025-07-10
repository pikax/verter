<script setup lang="ts">
defineProps({
  name: {
    type: String,
    required: true
  },
  age: Number,
});
const value = defineModel<string>();

const supa = defineModel('supa', {
  type: String,
  default: 'supa',
});

defineEmits({
  foo: (a: number) => true,
});

defineOptions({
  data() {
    return {
      stuff: "aaa",
    };
  },
});
</script>
<template>
  <span>{{ $props }}</span>
  <span>{{ $props.name }}</span>
  <span>{{ $props.modelValue }}</span>
  <span>{{ $props.onFoo}}</span>
  <span>{{ $emit("update:modelValue", 'a') }} </span>
  <span>{{ name }}</span>
  <span>{{ value }}</span>
  <span>{{ modelValue }}</span>
  <span>{{ $data }}</span>
</template>
