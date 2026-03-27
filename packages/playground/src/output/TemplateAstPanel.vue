<script setup lang="ts">
import { computed, ref, defineComponent, h, type PropType } from "vue";
import type { Store } from "../core/store";
import type { AnalysisTemplateElement } from "../core/types";

const props = defineProps<{
  store: Store;
}>();

const filterText = ref("");
const componentsOnly = ref(false);
const defaultExpandDepth = 3;

const elements = computed(() => {
  return props.store.activeFile?.compiled.analysis?.template?.elements ?? [];
});

interface TreeNode {
  element: AnalysisTemplateElement;
  index: number;
  children: TreeNode[];
  depth: number;
}

const tree = computed<TreeNode[]>(() => {
  const els = elements.value;
  if (!els.length) return [];

  const childMap = new Map<number | null, TreeNode[]>();
  const nodes: TreeNode[] = [];

  for (let i = 0; i < els.length; i++) {
    const el = els[i]!;
    const node: TreeNode = { element: el, index: i, children: [], depth: el.nestingDepth };
    nodes.push(node);

    const parentIdx = el.parentIndex ?? null;
    let siblings = childMap.get(parentIdx);
    if (!siblings) {
      siblings = [];
      childMap.set(parentIdx, siblings);
    }
    siblings.push(node);
  }

  for (const node of nodes) {
    node.children = childMap.get(node.index) ?? [];
  }

  return childMap.get(null) ?? [];
});

const filteredTree = computed<TreeNode[]>(() => {
  const filter = filterText.value.toLowerCase();
  const compOnly = componentsOnly.value;

  if (!filter && !compOnly) return tree.value;

  function matches(node: TreeNode): boolean {
    const el = node.element;
    if (compOnly && !el.isComponent) {
      return node.children.some(matches);
    }
    if (filter && !el.tag.toLowerCase().includes(filter)) {
      return node.children.some(matches);
    }
    return true;
  }

  function filterNodes(nodes: TreeNode[]): TreeNode[] {
    return nodes.filter(matches).map((n) => ({ ...n, children: filterNodes(n.children) }));
  }

  return filterNodes(tree.value);
});

const expanded = ref(new Set<number>());

function isExpanded(index: number, depth: number): boolean {
  if (expanded.value.has(index)) return true;
  return depth < defaultExpandDepth;
}

function toggleExpand(index: number) {
  if (expanded.value.has(index)) {
    expanded.value.delete(index);
  } else {
    expanded.value.add(index);
  }
}

function handleClick(el: AnalysisTemplateElement) {
  if (el.spanStart != null && el.spanEnd != null) {
    props.store.requestRevealSpan(el.spanStart, el.spanEnd);
  }
}

function directiveBadges(el: AnalysisTemplateElement): string[] {
  const badges: string[] = [];
  if (el.hasVIf) badges.push("v-if");
  if (el.hasVElseIf) badges.push("v-else-if");
  if (el.hasVElse) badges.push("v-else");
  if (el.hasVShow) badges.push("v-show");
  if (el.vFor) badges.push("v-for");
  if (el.vModel) badges.push("v-model");
  if (el.hasVHtml) badges.push("v-html");
  if (el.hasVText) badges.push("v-text");
  return badges;
}

function attrSummary(el: AnalysisTemplateElement): string {
  const parts: string[] = [];
  for (const attr of el.attributes ?? []) {
    if (attr.isDynamic) {
      parts.push(`:${attr.name}`);
    } else if (attr.value != null) {
      parts.push(`${attr.name}="${attr.value}"`);
    } else {
      parts.push(attr.name);
    }
  }
  return parts.slice(0, 4).join(" ") + (parts.length > 4 ? " ..." : "");
}

// Recursive tree node component (render function to support self-referencing)
const AstNode = defineComponent({
  name: "AstNode",
  props: {
    node: { type: Object as PropType<TreeNode>, required: true },
    isExpanded: {
      type: Function as PropType<(index: number, depth: number) => boolean>,
      required: true,
    },
    toggleExpand: { type: Function as PropType<(index: number) => void>, required: true },
    handleClick: {
      type: Function as PropType<(el: AnalysisTemplateElement) => void>,
      required: true,
    },
    directiveBadges: {
      type: Function as PropType<(el: AnalysisTemplateElement) => string[]>,
      required: true,
    },
    attrSummary: {
      type: Function as PropType<(el: AnalysisTemplateElement) => string>,
      required: true,
    },
  },
  setup(nodeProps) {
    return () => {
      const { node } = nodeProps;
      const el = node.element;
      const hasChildren = node.children.length > 0;
      const exp = nodeProps.isExpanded(node.index, node.depth);
      const badges = nodeProps.directiveBadges(el);
      const attrs = nodeProps.attrSummary(el);
      const indent = node.depth * 16;

      const children: ReturnType<typeof h>[] = [];
      const tagLabel = el.tag || "(unknown)";

      children.push(
        h(
          "div",
          {
            class: ["ast-node", el.isComponent ? "component" : "element"],
            style: { paddingLeft: `${indent + 4}px` },
            onClick: () => nodeProps.handleClick(el),
          },
          [
            hasChildren
              ? h(
                  "span",
                  {
                    class: ["ast-toggle-icon", exp ? "expanded" : ""],
                    onClick: (e: Event) => {
                      e.stopPropagation();
                      nodeProps.toggleExpand(node.index);
                    },
                  },
                  "\u25B6",
                )
              : h("span", { class: "ast-toggle-icon placeholder" }, " "),
            h(
              "span",
              { class: el.isComponent ? "tag-component" : "tag-html" },
              el.isSelfClosing ? `<${tagLabel} />` : `<${tagLabel}>`,
            ),
            attrs ? h("span", { class: "attr-summary" }, ` ${attrs}`) : null,
            ...badges.map((b: string) => h("span", { class: "directive-badge" }, b)),
            ...(el.dynamicClasses ?? [])
              .slice(0, 2)
              .map((c: string) => h("span", { class: "class-badge" }, `.${c}`)),
          ],
        ),
      );

      if (exp && hasChildren) {
        for (const child of node.children) {
          children.push(
            h(AstNode, {
              node: child,
              isExpanded: nodeProps.isExpanded,
              toggleExpand: nodeProps.toggleExpand,
              handleClick: nodeProps.handleClick,
              directiveBadges: nodeProps.directiveBadges,
              attrSummary: nodeProps.attrSummary,
            }),
          );
        }
      }

      return h("div", { class: "ast-node-group" }, children);
    };
  },
});
</script>

<template>
  <div class="ast-panel">
    <div class="ast-toolbar">
      <input v-model="filterText" class="ast-filter" type="text" placeholder="Filter elements..." />
      <label class="ast-toggle">
        <input v-model="componentsOnly" type="checkbox" />
        Components only
      </label>
    </div>

    <div v-if="!elements.length" class="empty-state">No template elements available</div>

    <div v-else class="ast-tree">
      <template v-for="node in filteredTree" :key="node.index">
        <AstNode
          :node="node"
          :is-expanded="isExpanded"
          :toggle-expand="toggleExpand"
          :handle-click="handleClick"
          :directive-badges="directiveBadges"
          :attr-summary="attrSummary"
        />
      </template>
    </div>
  </div>
</template>

<style scoped>
.ast-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
  font-family: ui-monospace, "Cascadia Code", "Source Code Pro", Menlo, Consolas, monospace;
}

.ast-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.ast-filter {
  flex: 1;
  padding: 4px 8px;
  font-size: 12px;
  background: var(--bg-primary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 3px;
  outline: none;
}

.ast-filter:focus {
  border-color: var(--accent-color, #4299e1);
}

.ast-toggle {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
}

.ast-toggle input {
  margin: 0;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-secondary);
  font-style: italic;
}

.ast-tree {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.ast-node {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 4px;
  cursor: pointer;
  border-radius: 2px;
  white-space: nowrap;
}

.ast-node:hover {
  background: var(--bg-tertiary);
}

.ast-toggle-icon {
  display: inline-block;
  width: 12px;
  font-size: 9px;
  color: var(--text-secondary);
  transition: transform 0.1s;
  cursor: pointer;
  text-align: center;
  flex-shrink: 0;
}

.ast-toggle-icon.expanded {
  transform: rotate(90deg);
}

.ast-toggle-icon.placeholder {
  visibility: hidden;
}

.tag-html {
  color: var(--accent-color, #4299e1);
}

.tag-component {
  color: #42b883;
  font-weight: 600;
}

.attr-summary {
  color: var(--text-secondary);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.directive-badge {
  padding: 1px 4px;
  font-size: 9px;
  font-weight: 600;
  background: rgba(168, 85, 247, 0.15);
  color: #a855f7;
  border-radius: 3px;
  flex-shrink: 0;
}

.class-badge {
  padding: 1px 4px;
  font-size: 9px;
  background: rgba(66, 153, 225, 0.15);
  color: var(--accent-color, #4299e1);
  border-radius: 3px;
  flex-shrink: 0;
}
</style>
