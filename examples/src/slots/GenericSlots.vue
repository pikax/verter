<script setup lang="ts" generic="T extends { id: number | string }">
// Generic component with typed slots

defineProps<{
  items: T[];
  keyField?: keyof T;
}>();

defineSlots<{
  default: (props: { item: T; index: number }) => any;
  header: (props: { count: number }) => any;
  empty: () => any;
  footer: (props: { items: T[] }) => any;
}>();
</script>

<template>
  <div class="generic-list">
    <header>
      <slot name="header" :count="items.length" />
    </header>

    <ul v-if="items.length > 0">
      <li v-for="(item, index) in items" :key="item.id">
        <slot :item="item" :index="index" />
      </li>
    </ul>

    <div v-else>
      <slot name="empty" />
    </div>

    <footer>
      <slot name="footer" :items="items" />
    </footer>
  </div>
</template>
