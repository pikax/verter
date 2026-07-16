<script setup lang="ts">
/**
 * Three-level fallthrough chain: OuterWrap → MidWrap → LeafNative → <button>.
 * This is the Verter differentiator pack: deep root inheritance across component roots.
 */
import MidWrap from "./MidWrap.vue";

defineProps<{ tone: string }>();
</script>

<template>
  <MidWrap :variant="tone" />
</template>
