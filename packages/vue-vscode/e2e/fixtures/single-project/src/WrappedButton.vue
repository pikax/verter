<script setup lang="ts">
import BaseButton from "./BaseButton.vue";
defineProps<{ variant: string }>();
</script>
<template><BaseButton :label="variant" /></template>
