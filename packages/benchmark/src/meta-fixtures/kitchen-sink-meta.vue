<script setup lang="ts">
import { ref, computed, watch, type Ref } from 'vue'

// ─── Complex Types ──────────────────────────────────────────────────────────

interface TreeNode {
  id: string
  label: string
  children?: TreeNode[]
  icon?: string
  disabled?: boolean
  data?: Record<string, unknown>
}

interface SelectionState {
  selected: Set<string>
  expanded: Set<string>
  focused: string | null
}

type FilterFn = (node: TreeNode, query: string) => boolean
type SortFn = (a: TreeNode, b: TreeNode) => number
type DragDropMode = 'none' | 'move' | 'copy' | 'link'

interface TreeViewConfig {
  multiSelect: boolean
  showCheckboxes: boolean
  showIcons: boolean
  virtualScroll: boolean
  lazyLoad: boolean
  dragDrop: DragDropMode
  filter: FilterFn | null
  sort: SortFn | null
}

// ─── Props ──────────────────────────────────────────────────────────────────

const props = withDefaults(
  defineProps<{
    /** Root nodes of the tree. */
    nodes: TreeNode[]
    /** Optional configuration overrides. */
    config?: Partial<TreeViewConfig>
    /** Maximum depth for tree expansion. */
    maxDepth?: number
    /** Whether the tree is in loading state. */
    loading?: boolean
    /** Placeholder text when tree is empty. */
    emptyText?: string
    /** Custom class for the tree container. */
    containerClass?: string | string[] | Record<string, boolean>
    /** Whether to animate expand/collapse. */
    animated?: boolean
    /** Debounce delay for filter input (ms). */
    filterDebounce?: number
    /** Accessible label for the tree. */
    ariaLabel?: string
    /** Whether nodes can be renamed inline. */
    editable?: boolean
  }>(),
  {
    maxDepth: 10,
    loading: false,
    emptyText: 'No items',
    animated: true,
    filterDebounce: 300,
    ariaLabel: 'Tree view',
    editable: false,
  },
)

// ─── Events ─────────────────────────────────────────────────────────────────

const emit = defineEmits<{
  /** Emitted when node selection changes. */
  'update:selection': [ids: string[]]
  /** Emitted when a node is expanded. */
  expand: [node: TreeNode]
  /** Emitted when a node is collapsed. */
  collapse: [node: TreeNode]
  /** Emitted when a node is clicked. */
  'node-click': [node: TreeNode, event: MouseEvent]
  /** Emitted when a node is double-clicked for editing. */
  'node-edit': [node: TreeNode, newLabel: string]
  /** Emitted on drag-and-drop between nodes. */
  'node-drop': [source: TreeNode, target: TreeNode, mode: DragDropMode]
}>()

// ─── Models ─────────────────────────────────────────────────────────────────

const selectedIds = defineModel<string[]>('selection', { default: () => [] })
const expandedIds = defineModel<string[]>('expanded', { default: () => [] })
const filterQuery = defineModel<string>('filter', { default: '' })

// ─── Slots ──────────────────────────────────────────────────────────────────

defineSlots<{
  /** Custom node label renderer. */
  'node-label'(props: { node: TreeNode; depth: number; isSelected: boolean; isExpanded: boolean }): any
  /** Custom icon renderer. */
  'node-icon'(props: { node: TreeNode; isExpanded: boolean }): any
  /** Loading indicator. */
  loading(props: { depth: number }): any
  /** Empty state. */
  empty(props: { filterQuery: string }): any
}>()

// ─── Internal State ─────────────────────────────────────────────────────────

const treeRef = ref<HTMLElement | null>(null)
const editingNode: Ref<string | null> = ref(null)

const flatNodes = computed(() => {
  const result: Array<{ node: TreeNode; depth: number }> = []
  function walk(nodes: TreeNode[], depth: number) {
    if (depth > props.maxDepth) return
    for (const node of nodes) {
      result.push({ node, depth })
      if (node.children && expandedIds.value.includes(node.id)) {
        walk(node.children, depth + 1)
      }
    }
  }
  walk(props.nodes, 0)
  return result
})

const visibleCount = computed(() => flatNodes.value.length)

watch(filterQuery, (query) => {
  if (!query) return
  // Auto-expand parents of matching nodes
  const matching = new Set<string>()
  function walk(nodes: TreeNode[], ancestors: string[]) {
    for (const node of nodes) {
      const config = props.config
      const matches = config?.filter
        ? config.filter(node, query)
        : node.label.toLowerCase().includes(query.toLowerCase())
      if (matches) {
        for (const a of ancestors) matching.add(a)
      }
      if (node.children) walk(node.children, [...ancestors, node.id])
    }
  }
  walk(props.nodes, [])
  expandedIds.value = [...new Set([...expandedIds.value, ...matching])]
})

// ─── Expose ─────────────────────────────────────────────────────────────────

function expandAll() {
  const ids: string[] = []
  function walk(nodes: TreeNode[]) {
    for (const n of nodes) {
      ids.push(n.id)
      if (n.children) walk(n.children)
    }
  }
  walk(props.nodes)
  expandedIds.value = ids
}

function collapseAll() {
  expandedIds.value = []
}

function selectAll() {
  selectedIds.value = flatNodes.value.map(f => f.node.id)
}

function deselectAll() {
  selectedIds.value = []
}

defineExpose({ expandAll, collapseAll, selectAll, deselectAll })
</script>

<template>
  <div
    ref="treeRef"
    :class="['tree-view', containerClass, { loading, animated }]"
    :aria-label="ariaLabel"
    role="tree"
  >
    <template v-if="visibleCount === 0 && !loading">
      <slot name="empty" :filter-query="filterQuery">
        <p class="empty-text">{{ emptyText }}</p>
      </slot>
    </template>

    <template v-else>
      <div
        v-for="{ node, depth } in flatNodes"
        :key="node.id"
        :style="{ paddingLeft: depth * 20 + 'px' }"
        :class="['tree-node', { selected: selectedIds.includes(node.id), disabled: node.disabled }]"
        :aria-expanded="node.children ? expandedIds.includes(node.id) : undefined"
        role="treeitem"
        @click="emit('node-click', node, $event)"
        @dblclick="editable && (editingNode = node.id)"
      >
        <slot name="node-icon" :node="node" :is-expanded="expandedIds.includes(node.id)">
          <span v-if="node.icon" class="icon">{{ node.icon }}</span>
        </slot>

        <slot
          name="node-label"
          :node="node"
          :depth="depth"
          :is-selected="selectedIds.includes(node.id)"
          :is-expanded="expandedIds.includes(node.id)"
        >
          <span class="label">{{ node.label }}</span>
        </slot>
      </div>
    </template>

    <template v-if="loading">
      <slot name="loading" :depth="0">
        <div class="loading-indicator">Loading...</div>
      </slot>
    </template>
  </div>
</template>

<style scoped>
.tree-view {
  font-family: sans-serif;
  user-select: none;
}
.tree-node {
  padding: 4px 8px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 4px;
}
.tree-node:hover {
  background: #f0f0f0;
}
.tree-node.selected {
  background: #e3f2fd;
}
.tree-node.disabled {
  opacity: 0.5;
  pointer-events: none;
}
.empty-text {
  color: #999;
  text-align: center;
  padding: 16px;
}
.loading-indicator {
  text-align: center;
  padding: 16px;
  color: #666;
}
</style>

<style module>
.treeContainer {
  position: relative;
  overflow: auto;
}
</style>
