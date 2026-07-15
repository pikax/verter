<script setup lang="ts" generic="T extends Record<string, any>">
import type { TableColumn, TableRowClick } from './table_types';

defineProps<{
  columns: TableColumn<T>[];
  rows: T[];
  loading?: boolean;
}>();

defineEmits<{
  'row-click': [event: TableRowClick<T>];
}>();
</script>

<template>
  <div class="table" :class="{ loading }">
    <div v-if="loading" class="loading-anim">…</div>
    <table v-else>
      <thead>
        <tr><th v-for="c in columns" :key="c.key">{{ c.label }}</th></tr>
      </thead>
      <tbody>
        <tr v-for="(row, i) in rows" :key="i" @click="$emit('row-click', { row, index: i })">
          <td v-for="c in columns" :key="c.key">{{ row[c.key] }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
