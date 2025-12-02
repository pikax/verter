<script setup lang="ts">
import type { Directive, DirectiveBinding } from "vue";

// Simple directive - just the mounted hook
const vFocus: Directive<HTMLInputElement> = {
  mounted(el) {
    el.focus();
  },
};

// Full directive with all hooks
const vHighlight: Directive<HTMLElement, string> = {
  created(el, binding) {
    // Called before element's attributes or event listeners are applied
  },
  beforeMount(el, binding) {
    // Called before element is inserted into the DOM
  },
  mounted(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
  beforeUpdate(el, binding) {
    // Called before the containing component's VNode is updated
  },
  updated(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
  beforeUnmount(el, binding) {
    // Called before the element is removed from the DOM
  },
  unmounted(el, binding) {
    // Called after the element is removed from the DOM
  },
};

// Directive with modifiers
const vClickOutside: Directive<HTMLElement, () => void> = {
  mounted(el, binding) {
    const handler = (event: MouseEvent) => {
      if (!el.contains(event.target as Node)) {
        binding.value();
      }
    };
    (el as any).__clickOutsideHandler = handler;
    document.addEventListener("click", handler);
  },
  unmounted(el) {
    document.removeEventListener("click", (el as any).__clickOutsideHandler);
  },
};

// Directive with argument
const vTooltip: Directive<HTMLElement, string> = {
  mounted(el, binding: DirectiveBinding<string>) {
    const position = binding.arg || "top"; // v-tooltip:top, v-tooltip:bottom
    const text = binding.value;
    el.setAttribute("data-tooltip", text);
    el.setAttribute("data-tooltip-position", position);
  },
  updated(el, binding: DirectiveBinding<string>) {
    el.setAttribute("data-tooltip", binding.value);
  },
};

// Directive with complex value type
interface ResizeOptions {
  minWidth?: number;
  maxWidth?: number;
  onResize?: (width: number) => void;
}

const vResize: Directive<HTMLElement, ResizeOptions> = {
  mounted(el, binding) {
    const { minWidth = 0, maxWidth = Infinity, onResize } = binding.value || {};
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const width = Math.min(Math.max(entry.contentRect.width, minWidth), maxWidth);
        onResize?.(width);
      }
    });
    observer.observe(el);
    (el as any).__resizeObserver = observer;
  },
  unmounted(el) {
    (el as any).__resizeObserver?.disconnect();
  },
};

// Function shorthand (mounted + updated)
const vColor: Directive<HTMLElement, string> = (el, binding) => {
  el.style.color = binding.value;
};

function handleClickOutside() {
  console.log("Clicked outside!");
}

function handleResize(width: number) {
  console.log("Resized to:", width);
}
</script>

<template>
  <div>
    <h2>Custom Directives</h2>

    <!-- v-focus -->
    <input v-focus placeholder="Auto-focused" />

    <!-- v-highlight with value -->
    <p v-highlight="'lightblue'">Highlighted text</p>

    <!-- v-highlight with modifiers -->
    <p v-highlight.important="'pink'">Important highlight</p>

    <!-- v-click-outside -->
    <div v-click-outside="handleClickOutside" class="box">
      Click outside me
    </div>

    <!-- v-tooltip with argument -->
    <button v-tooltip:top="'This is a tooltip'">Hover me</button>
    <button v-tooltip:bottom="'Bottom tooltip'">Bottom</button>

    <!-- v-resize with complex options -->
    <div v-resize="{ minWidth: 100, maxWidth: 500, onResize: handleResize }">
      Resizable content
    </div>

    <!-- v-color function shorthand -->
    <span v-color="'red'">Red text</span>
  </div>
</template>
