<template>
  <span>{{ itemLabel }}</span>
  <span>{{ sameItem.id }}</span>
</template>

<script setup lang="ts">
interface DrawerItem {
  id: number;
  label: string;
}

const item: DrawerItem = { id: 1, label: "row" };

// A complete member access: completion/hover at the end of `item.label` resolves
// the `label` member off `DrawerItem`.
const itemLabel = item.label; // @dx-anchor mma.member

// An incomplete-expression site: the scenario types `.` after this reference, then
// completion must surface the `DrawerItem` members. The bare reference is valid.
const sameItem = item; // @dx-anchor mma.incomplete
</script>
