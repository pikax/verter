<script setup lang="ts">
/**
 * A comprehensive data table component with sorting, filtering and pagination.
 *
 * @example
 * ```vue
 * <DataTable :columns="cols" :rows="data" :page-size="20" />
 * ```
 */

/** Column definition for the data table. */
interface Column {
  /** Unique key for the column */
  key: string
  /** Display label */
  label: string
  /** Whether the column is sortable */
  sortable?: boolean
  /** Column width in pixels */
  width?: number
}

/** Sort direction */
type SortDir = 'asc' | 'desc' | 'none'

/**
 * @property columns - Column definitions
 * @property rows - Data rows
 * @property pageSize - Items per page
 * @property currentPage - Active page number (1-based)
 * @property sortBy - Column key to sort by
 * @property loading - Whether data is loading
 */
const props = defineProps<{
  /** Table column definitions. */
  columns: Column[]
  /** Table row data. */
  rows: Record<string, unknown>[]
  /** Number of rows per page. @default 10 */
  pageSize?: number
  /** Current page number (1-based). */
  currentPage?: number
  /** Column key to sort by. */
  sortBy?: string
  /** Whether the table is in loading state. */
  loading?: boolean
}>()

/**
 * @event sort - Fired when a column header is clicked for sorting.
 * @event page-change - Fired when the user navigates to a different page.
 * @event row-click - Fired when a row is clicked.
 * @event select - Fired when rows are selected/deselected.
 */
const emit = defineEmits<{
  /** Emitted when sorting changes. */
  sort: [column: string, direction: SortDir]
  /** Emitted when page changes. */
  'page-change': [page: number]
  /** Emitted when a row is clicked. */
  'row-click': [row: Record<string, unknown>, index: number]
  /** Emitted when selection changes. */
  select: [selectedRows: Record<string, unknown>[]]
}>()

/**
 * @slot header - Custom header content above the table.
 * @slot cell - Custom cell renderer for individual cells.
 */
defineSlots<{
  /** Custom header content. */
  header(props: { totalRows: number; currentPage: number }): any
  /** Custom cell content. */
  cell(props: { column: Column; row: Record<string, unknown>; value: unknown }): any
}>()

const page = defineModel<number>('currentPage', { default: 1 })

/** Refresh the table data. */
function refresh() {
  emit('page-change', page.value)
}

/** Select all visible rows. */
function selectAll() {
  emit('select', props.rows)
}

defineExpose({
  /** Refresh table data from the current page. */
  refresh,
  /** Select all currently visible rows. */
  selectAll,
})
</script>

<template>
  <div class="data-table" :class="{ loading }">
    <slot name="header" :total-rows="rows.length" :current-page="page" />
    <table>
      <thead>
        <tr>
          <th
            v-for="col in columns"
            :key="col.key"
            :style="col.width ? { width: col.width + 'px' } : undefined"
            @click="col.sortable && emit('sort', col.key, 'asc')"
          >
            {{ col.label }}
          </th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="(row, i) in rows"
          :key="i"
          @click="emit('row-click', row, i)"
        >
          <td v-for="col in columns" :key="col.key">
            <slot name="cell" :column="col" :row="row" :value="row[col.key]">
              {{ row[col.key] }}
            </slot>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style module>
.data-table {
  width: 100%;
  border-collapse: collapse;
}
.data-table.loading {
  opacity: 0.5;
  pointer-events: none;
}
</style>
