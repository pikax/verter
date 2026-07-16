<script setup lang="ts">
defineSlots<{
  header(props: { title: string }): any;
  default(props: { body: string }): any;
}>();
</script>

<template>
  <header>
    <slot name="header" title="hdr" />
  </header>
  <main>
    <slot body="main-body" />
  </main>
</template>
