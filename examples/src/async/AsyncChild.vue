<script setup lang="ts">
// Async child component stub
defineProps<{
  message?: string;
}>();

defineModel<string>("foo", {
  required: false,
  default: "",
});

</script>

<template>
  <div class="async-child">
    <p>{{ message ?? "Async Child Component" }}</p>
    <p>{{ foo }}</p>

    <p>{{ $props }}</p>

    <p>{{ }}</p>

    <p>{{ {} as ___VERTER___TemplateBinding }}</p>
  </div>
</template>
