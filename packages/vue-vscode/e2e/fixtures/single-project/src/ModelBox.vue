<script setup lang="ts">
// A defineModel whose payload is a NESTED object type: `size` is NOT a prop
// field (the macro's prop is `modelValue`), so navigating to it is
// provider-only — no native leg can serve it — and IDE codegen RELOCATES the
// inline object-literal type text into the Prettify-wrapped macro type alias,
// making the reverse map over MOVED authored text load-bearing.
const box = defineModel<{ size: number }>({ required: true });
const wide = box.value.size > 1;
</script>
<template>
  <p v-if="wide">{{ box.size }}</p>
</template>
