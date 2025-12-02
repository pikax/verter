<script lang="ts">
// Functional component patterns for parser testing - Options API + TypeScript
// Demonstrates functional-style patterns in Options API

import { defineComponent, h, type PropType, type FunctionalComponent, type VNode } from "vue";

// Define functional component types
interface FunctionalButtonProps {
  title: string;
  count?: number;
  onClick?: () => void;
}

// Standalone functional component
export const StandaloneFunctional: FunctionalComponent<FunctionalButtonProps> = (
  props,
  { slots, attrs }
) => {
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
  onClick: { type: Function as PropType<() => void>, default: undefined },
};

// Main component using Options API with functional patterns
export default defineComponent({
  name: "FunctionalObjectTs",

  // Explicitly functional: false (default in Vue 3, but shown for clarity)
  // In Vue 2, there was a `functional: true` option

  props: {
    message: {
      type: String as PropType<string>,
      required: true,
    },
    items: {
      type: Array as PropType<Array<{ id: string; label: string }>>,
      default: () => [],
    },
    variant: {
      type: String as PropType<"primary" | "secondary" | "danger">,
      default: "primary",
    },
    disabled: {
      type: Boolean as PropType<boolean>,
      default: false,
    },
  },

  emits: {
    click: (item: { id: string; label: string }) => true,
    hover: (id: string) => true,
  },

  // No data() - stateless pattern
  // Using only computed properties derived from props

  computed: {
    formattedMessage(): string {
      return this.message.toUpperCase();
    },

    itemCount(): number {
      return this.items.length;
    },

    variantClass(): string {
      return `btn-${this.variant}`;
    },

    buttonClasses(): Record<string, boolean> {
      return {
        [this.variantClass]: true,
        disabled: this.disabled,
      };
    },
  },

  methods: {
    // Pure handlers - just emit, no local state mutation
    handleItemClick(item: { id: string; label: string }): void {
      this.$emit("click", item);
    },

    handleItemHover(id: string): void {
      this.$emit("hover", id);
    },

    // Render helper methods for complex rendering logic
    renderItem(item: { id: string; label: string }): VNode {
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

  // Optional render function for fully programmatic rendering
  // Uncomment to use instead of template
  /*
  render() {
    return h("div", { class: "functional-demo" }, [
      h("h2", this.formattedMessage),
      h("p", `Item count: ${this.itemCount}`),
      h("ul", this.items.map((item) => this.renderItem(item))),
      h(
        "button",
        {
          class: this.buttonClasses,
          disabled: this.disabled,
        },
        `${this.variant} button`
      ),
      this.$slots.footer?.({ count: this.itemCount }),
    ]);
  },
  */
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
