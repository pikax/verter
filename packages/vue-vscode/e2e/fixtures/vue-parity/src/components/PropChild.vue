<script setup lang="ts">
defineProps<{
  contractProp: string;
  optionalFlag?: boolean;
}>();
</script>

<template>
  <span>{{ contractProp }}</span>
</template>
