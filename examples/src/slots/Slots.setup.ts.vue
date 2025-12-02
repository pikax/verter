<script setup lang="ts">
// Slots definition
defineSlots<{
  default: (props: { item: string }) => any;
  header: (props: { title: string }) => any;
  footer: () => any;
}>();

const items = ["Item 1", "Item 2", "Item 3"];
</script>

<template>
  <div>
    <header>
      <slot name="header" :title="'My Title'" />
    </header>
    <main>
      <div v-for="item in items" :key="item">
        <slot :item="item" />
      </div>
    </main>
    <footer>
      <slot name="footer" />
    </footer>
  </div>
</template>
