<script setup lang="ts">
/**
 * Single component root → LeafNative.
 * Declares only `variant`. Deep fallthrough should accept attrs not declared here
 * and forward them through LeafNative onto the native button.
 */
import LeafNative from "./LeafNative.vue";

defineProps<{ variant: string }>();
</script>

<template>
  <LeafNative :label="variant" />
</template>
