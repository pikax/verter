<script setup lang="ts">
import { ref } from "vue";
import type { Directive, DirectiveBinding, ObjectDirective, FunctionDirective } from "vue";

// Function directive - simple form
const vFocus: FunctionDirective<HTMLElement> = (el) => {
  el.focus();
};

// Object directive with all hooks
const vHighlight: ObjectDirective<HTMLElement, string> = {
  created(el, binding) {
    // Called before element's attributes or event listeners are applied
  },
  beforeMount(el, binding) {
    // Called right before element is inserted into DOM
  },
  mounted(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
  beforeUpdate(el, binding) {
    // Called before containing component's VNode is updated
  },
  updated(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
  beforeUnmount(el, binding) {
    // Called before element is removed from DOM
  },
  unmounted(el, binding) {
    // Called when directive's parent component is unmounted
  },
};

// Directive with modifiers
interface ClickOutsideBinding {
  handler: (event: MouseEvent) => void;
  exclude?: string[];
}

const vClickOutside: Directive<HTMLElement, ClickOutsideBinding> = {
  mounted(el, binding) {
    const handler = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      const excluded = binding.value.exclude || [];
      const isExcluded = excluded.some((selector) =>
        target.closest(selector)
      );
      if (!el.contains(target) && !isExcluded) {
        binding.value.handler(event);
      }
    };
    document.addEventListener("click", handler);
    (el as any).__clickOutsideHandler = handler;
  },
  unmounted(el) {
    document.removeEventListener("click", (el as any).__clickOutsideHandler);
  },
};

// Directive with argument
const vTooltip: Directive<HTMLElement, string> = {
  mounted(el, binding) {
    const position = binding.arg || "top"; // 'top' | 'bottom' | 'left' | 'right'
    const text = binding.value;
    el.setAttribute("data-tooltip", text);
    el.setAttribute("data-tooltip-position", position);
  },
  updated(el, binding) {
    el.setAttribute("data-tooltip", binding.value);
  },
};

// Directive using modifiers
const vDraggable: Directive<HTMLElement, void> = {
  mounted(el, binding) {
    const { modifiers } = binding;
    if (modifiers.x && !modifiers.y) {
      // Only horizontal drag
      el.setAttribute("data-drag-axis", "x");
    } else if (modifiers.y && !modifiers.x) {
      // Only vertical drag
      el.setAttribute("data-drag-axis", "y");
    } else {
      el.setAttribute("data-drag-axis", "both");
    }
    if (modifiers.bounded) {
      el.setAttribute("data-drag-bounded", "true");
    }
    el.style.cursor = "move";
  },
};

// Directive with typed binding
interface PermissionBinding {
  role: string;
  action: string;
}

const vPermission: Directive<HTMLElement, PermissionBinding> = {
  mounted(el, binding: DirectiveBinding<PermissionBinding>) {
    const { role, action } = binding.value;
    // Check permission logic
    const hasPermission = checkPermission(role, action);
    if (!hasPermission) {
      el.style.display = "none";
    }
  },
};

function checkPermission(role: string, action: string): boolean {
  // Placeholder permission check
  return true;
}

// Reactive state for demo
const isDropdownOpen = ref(false);
const highlightColor = ref("yellow");
const tooltipText = ref("Hover me!");

function closeDropdown() {
  isDropdownOpen.value = false;
}

function toggleDropdown() {
  isDropdownOpen.value = !isDropdownOpen.value;
}
</script>

<template>
  <div class="directives-demo">
    <!-- v-focus directive -->
    <input v-focus type="text" placeholder="Auto-focused" />

    <!-- v-highlight with binding value -->
    <div v-highlight="highlightColor">Highlighted text</div>
    <button @click="highlightColor = highlightColor === 'yellow' ? 'cyan' : 'yellow'">
      Toggle Color
    </button>

    <!-- v-click-outside with handler -->
    <div
      v-click-outside="{ handler: closeDropdown, exclude: ['.dropdown-toggle'] }"
      class="dropdown"
    >
      <button class="dropdown-toggle" @click="toggleDropdown">
        Toggle Dropdown
      </button>
      <div v-if="isDropdownOpen" class="dropdown-menu">
        Dropdown content
      </div>
    </div>

    <!-- v-tooltip with argument -->
    <span v-tooltip:top="tooltipText">Hover for top tooltip</span>
    <span v-tooltip:bottom="'Bottom tooltip'">Hover for bottom tooltip</span>

    <!-- v-draggable with modifiers -->
    <div v-draggable.x.bounded class="draggable">Drag horizontal only</div>
    <div v-draggable.y class="draggable">Drag vertical only</div>
    <div v-draggable class="draggable">Drag any direction</div>

    <!-- v-permission with object binding -->
    <button v-permission="{ role: 'admin', action: 'delete' }">
      Admin Only Button
    </button>
  </div>
</template>
