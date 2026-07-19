<template>
  <section>
    <template #head>
      <slot name="head"></slot>
    </template>
    {{ title }}
  </section>
</template>

<script setup lang="ts" generic="T extends { id: string }">
const props = defineProps<{ rows: T[]; title: string }>();
</script>
