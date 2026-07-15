<script setup lang="ts">
import type { ToolbarItem, ToolbarItemOrGroup } from './editor_toolbar_types';

defineProps<{
  items: ToolbarItemOrGroup[];
  size?: 'sm' | 'md' | 'lg';
}>();

defineEmits<{
  'item-click': [item: ToolbarItem];
}>();
</script>

<template>
  <div class="editor-toolbar" :class="size">
    <template v-for="(entry, i) in items" :key="i">
      <div v-if="Array.isArray(entry)" class="group">
        <button v-for="item in entry" :key="item.id" @click="$emit('item-click', item)">
          {{ item.label }}
        </button>
      </div>
      <button v-else @click="$emit('item-click', entry)">{{ entry.label }}</button>
    </template>
  </div>
</template>
