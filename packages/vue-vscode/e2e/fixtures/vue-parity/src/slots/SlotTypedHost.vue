<script setup lang="ts">
/**
 * Strict defineSlots host for positive + negative slot E2E.
 */
defineSlots<{
  header(props: { title: string; count: number }): any;
  default(props: { body: string; flag: boolean }): any;
  footer?(props: { ok: boolean }): any;
}>();
</script>

<template>
  <header>
    <slot name="header" title="hdr" :count="3" />
  </header>
  <main>
    <slot body="main-body" :flag="true" />
  </main>
  <footer>
    <slot name="footer" :ok="true" />
  </footer>
</template>
