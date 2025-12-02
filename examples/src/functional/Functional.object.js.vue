<script>
// Functional component patterns for parser testing - Options API + JavaScript
// Demonstrates functional-style patterns in Options API

import { defineComponent, h } from "vue";

// Standalone functional component (object style)
export const StandaloneFunctional = (props, { slots, attrs }) => {
  return h(
    "button",
    {
      class: "standalone-btn",
      onClick: props.onClick,
      ...attrs,
    },
    [props.title, props.count !== undefined ? ` (${props.count})` : "", slots.default?.()]
  );
};

StandaloneFunctional.props = {
  title: { type: String, required: true },
  count: { type: Number, default: undefined },
  onClick: { type: Function, default: undefined },
};

// Main component using Options API with functional patterns
export default defineComponent({
  name: "FunctionalObjectJs",

  props: {
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
  },

  emits: ["click", "hover"],

  // No data() - stateless pattern
  // Using only computed properties derived from props

  computed: {
    formattedMessage() {
      return this.message.toUpperCase();
    },

    itemCount() {
      return this.items.length;
    },

    variantClass() {
      return `btn-${this.variant}`;
    },

    buttonClasses() {
      return {
        [this.variantClass]: true,
        disabled: this.disabled,
      };
    },
  },

  methods: {
    // Pure handlers - just emit, no local state mutation
    handleItemClick(item) {
      this.$emit("click", item);
    },

    handleItemHover(id) {
      this.$emit("hover", id);
    },

    // Render helper methods for complex rendering logic
    renderItem(item) {
      return h(
        "li",
        {
          key: item.id,
          onClick: () => this.handleItemClick(item),
          onMouseenter: () => this.handleItemHover(item.id),
        },
        item.label
      );
    },
  },
});
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
      {{ variant }} button
    </button>

    <!-- Slot for composition -->
    <slot name="footer" :count="itemCount" />
  </div>
</template>
