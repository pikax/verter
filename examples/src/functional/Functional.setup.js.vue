<script setup>
// Functional component patterns for parser testing - Setup API + JavaScript
// Note: In Vue 3, all <script setup> components are effectively functional by default.
// This file demonstrates patterns commonly associated with functional/stateless components.

import { computed, h } from "vue";

// Props-only component (no local state) - pure rendering
const props = defineProps({
  message: {
    type: String,
    required: true,
  },
  items: {
    type: Array,
    default: () => [],
  },
  variant: {
    type: String,
    default: "primary",
    validator: (v) => ["primary", "secondary", "danger"].includes(v),
  },
  disabled: {
    type: Boolean,
    default: false,
  },
});

// Pure computed values (derived from props only)
const formattedMessage = computed(() => props.message.toUpperCase());

const itemCount = computed(() => props.items.length);

const variantClass = computed(() => `btn-${props.variant ?? "primary"}`);

const buttonClasses = computed(() => ({
  [variantClass.value]: true,
  disabled: props.disabled,
}));

// Event emit for pure pass-through
const emit = defineEmits(["click", "hover"]);

// Pure function handlers (no side effects on local state)
function handleItemClick(item) {
  emit("click", item);
}

function handleItemHover(id) {
  emit("hover", id);
}
</script>

<script>
// Traditional functional component definition (Vue 3 style)
// This is kept separate as it's a different component definition pattern

import { h as createElement } from "vue";

// Functional component (object style)
export const FunctionalButton = (props, { slots, emit, attrs }) => {
  return createElement(
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
    type: Function,
    default: undefined,
  },
};

FunctionalButton.displayName = "FunctionalButton";

// Render function component with complex props
export const RenderList = (props, { slots }) => {
  const defaultRenderItem = (item) =>
    createElement("li", { key: item.id, class: { active: item.active } }, item.name);

  const renderFn = props.renderItem ?? defaultRenderItem;

  return createElement(
    "ul",
    { class: "render-list" },
    props.items.map(renderFn)
  );
};

RenderList.props = {
  items: {
    type: Array,
    required: true,
  },
  renderItem: {
    type: Function,
    default: undefined,
  },
};

// Factory function for creating typed list components
export function createTypedList() {
  const component = (props) => {
    return createElement("div", { class: "typed-list" }, props.items.map(props.renderItem));
  };

  component.props = {
    items: { type: Array, required: true },
    renderItem: { type: Function, required: true },
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
