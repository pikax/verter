<script setup lang="ts">
import { defineComponent } from "vue";

defineOptions({
  inheritAttrs: false,
});

defineProps<{
  modelValue?: any;

  required?: boolean;

  type: {};
}>();

defineEmits({
  resolve: () => true,
  pending: () => true,
  fallback: () => true,
});

const Editor = defineComponent({});
</script>

<template>
  <Suspense
    @resolve="$emit('resolve')"
    @fallback="$emit('fallback')"
    @pending="$emit('pending')"
  >
    <Editor v-bind="{ ...$props, ...$attrs }" />
  </Suspense>
</template>
