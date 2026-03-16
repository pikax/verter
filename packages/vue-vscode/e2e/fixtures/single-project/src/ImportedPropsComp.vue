<script setup lang="ts">
import type { ImportedProps } from "./importedProps"

const props = withDefaults(defineProps<ImportedProps>(), {
  title: "hello",
})
</script>

<template>
  <section>
    <h2>{{ title }}</h2>
    <p>{{ props.cou }}</p>
    <p>{{ props.count }}</p>
  </section>
</template>
