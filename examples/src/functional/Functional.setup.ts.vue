<script setup lang="ts">
// Functional component patterns for parser testing - Setup API + TypeScript
// Note: In Vue 3, all <script setup> components are effectively functional by default.
// This file demonstrates patterns commonly associated with functional/stateless components.

import { computed, type FunctionalComponent, type PropType, h, type VNode } from "vue";

// Props-only component (no local state) - pure rendering
const props = defineProps<{
  message: string;
  items: Array<{ id: string; label: string }>;
  variant?: "primary" | "secondary" | "danger";
  disabled?: boolean;
}>();

// Pure computed values (derived from props only)
const formattedMessage = computed(() => props.message.toUpperCase());

const itemCount = computed(() => props.items.length);

const variantClass = computed(() => `btn-${props.variant ?? "primary"}`);

const buttonClasses = computed(() => ({
  [variantClass.value]: true,
  disabled: props.disabled,
}));

// Event emit for pure pass-through
const emit = defineEmits<{
  (e: "click", item: { id: string; label: string }): void;
  (e: "hover", id: string): void;
}>();

// Pure function handlers (no side effects on local state)
function handleItemClick(item: { id: string; label: string }): void {
  emit("click", item);
}

function handleItemHover(id: string): void {
  emit("hover", id);
}
</script>

<script lang="ts">
// Traditional functional component definition (Vue 3 style)
// This is kept separate as it's a different component definition pattern

import { type FunctionalComponent as FC, type PropType as PT } from "vue";

// Typed functional component
interface FunctionalProps {
  title: string;
  count?: number;
  onClick?: () => void;
}

export const FunctionalButton: FC<FunctionalProps> = (props, { slots, emit, attrs }) => {
  return h(
    "button",
    {
      class: "functional-btn",
      onClick: props.onClick,
      ...attrs,
    },
    [
      props.title,
      props.count !== undefined ? ` (${props.count})` : "",
      slots.default?.(),
    ]
  );
};

FunctionalButton.props = {
  title: {
    type: String,
    required: true,
  },
  count: {
    type: Number,
    default: undefined,
  },
  onClick: {
    type: Function as PT<() => void>,
    default: undefined,
  },
};

FunctionalButton.displayName = "FunctionalButton";

// Render function component with complex props
interface RenderProps {
  items: Array<{ id: string; name: string; active: boolean }>;
  renderItem?: (item: { id: string; name: string; active: boolean }) => VNode;
}

export const RenderList: FC<RenderProps> = (props, { slots }) => {
  const defaultRenderItem = (item: typeof props.items[number]) =>
    h("li", { key: item.id, class: { active: item.active } }, item.name);

  const renderFn = props.renderItem ?? defaultRenderItem;

  return h(
    "ul",
    { class: "render-list" },
    props.items.map(renderFn)
  );
};

RenderList.props = {
  items: {
    type: Array as PT<RenderProps["items"]>,
    required: true,
  },
  renderItem: {
    type: Function as PT<RenderProps["renderItem"]>,
    default: undefined,
  },
};

// Generic-like functional component using factory
export function createTypedList<T extends { id: string | number }>(): FC<{
  items: T[];
  renderItem: (item: T) => VNode;
}> {
  const component: FC<{ items: T[]; renderItem: (item: T) => VNode }> = (props) => {
    return h("div", { class: "typed-list" }, props.items.map(props.renderItem));
  };

  component.props = {
    items: { type: Array as PT<T[]>, required: true },
    renderItem: { type: Function as PT<(item: T) => VNode>, required: true },
  };

  return component;
}
</script>

<template>
  <div class="functional-demo">
    <!-- Pure rendering based on props -->
    <h2>{{ formattedMessage }}</h2>
    <p>Item count: {{ itemCount }}</p>

    <!-- Stateless list rendering -->
    <ul>
      <li
        v-for="item in items"
        :key="item.id"
        @click="handleItemClick(item)"
        @mouseenter="handleItemHover(item.id)"
      >
        {{ item.label }}
      </li>
    </ul>

    <!-- Conditional rendering based on props only -->
    <button :class="buttonClasses" :disabled="disabled">
      {{ variant ?? "primary" }} button
    </button>

    <!-- Slot for composition -->
    <slot name="footer" :count="itemCount" />
  </div>
</template>
