<template>
  <article>{{ drawerTitle }} {{ drawerCount }}</article>
</template>

<script setup lang="ts">
interface DrawerProps {
  title: string;
  count?: number;
}

const props = defineProps<DrawerProps>();

const drawerTitle = props.title; // @dx-anchor props.title
const drawerCount = props.count; // @dx-anchor props.count
</script>
