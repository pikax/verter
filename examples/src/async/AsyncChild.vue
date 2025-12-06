<script setup lang="ts">
// Async child component stub
defineProps<{
  message?: string;
}>();
</script>

<template>
  <div class="async-child">
    <p>{{ message ?? "Async Child Component" }}</p>

    <p>{{ $props }}</p>

    <p>{{}}</p>

    <p>{{ {} as ___VERTER___TemplateBinding }}</p>
  </div>
</template>
