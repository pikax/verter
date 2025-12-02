<script setup lang="ts">
// defineSlots patterns for parser testing

// Basic typed slots with scoped slot props
defineSlots<{
  default: (props: { item: string; index: number }) => any;
  header: (props: { title: string }) => any;
  footer: () => any;
  empty: () => any;
}>();

const exampleItem = "example";
const exampleIndex = 0;
</script>

<template>
  <div>
    <!-- Named slots with props -->
    <header>
      <slot name="header" :title="'Page Title'" />
    </header>

    <main>
      <!-- Scoped slot with props -->
      <slot :item="exampleItem" :index="exampleIndex" />
    </main>

    <footer>
      <slot name="footer" />
    </footer>

    <div v-if="false">
      <slot name="empty" />
    </div>
  </div>
</template>
