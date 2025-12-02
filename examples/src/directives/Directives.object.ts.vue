<script lang="ts">
import { defineComponent, type Directive, type ObjectDirective, ref } from "vue";

// Define directives outside component for reuse
const focusDirective: ObjectDirective<HTMLElement> = {
  mounted(el) {
    el.focus();
  },
};

const highlightDirective: ObjectDirective<HTMLElement, string> = {
  mounted(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
  updated(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
};

interface ClickOutsideBinding {
  handler: (event: MouseEvent) => void;
  exclude?: string[];
}

const clickOutsideDirective: Directive<HTMLElement, ClickOutsideBinding> = {
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

const tooltipDirective: Directive<HTMLElement, string> = {
  mounted(el, binding) {
    const position = binding.arg || "top";
    el.setAttribute("data-tooltip", binding.value);
    el.setAttribute("data-tooltip-position", position);
  },
  updated(el, binding) {
    el.setAttribute("data-tooltip", binding.value);
  },
};

const draggableDirective: Directive<HTMLElement, void> = {
  mounted(el, binding) {
    const { modifiers } = binding;
    if (modifiers.x && !modifiers.y) {
      el.setAttribute("data-drag-axis", "x");
    } else if (modifiers.y && !modifiers.x) {
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

interface PermissionBinding {
  role: string;
  action: string;
}

function checkPermission(role: string, action: string): boolean {
  return true;
}

const permissionDirective: Directive<HTMLElement, PermissionBinding> = {
  mounted(el, binding) {
    const { role, action } = binding.value;
    if (!checkPermission(role, action)) {
      el.style.display = "none";
    }
  },
};

export default defineComponent({
  name: "Directives",
  directives: {
    focus: focusDirective,
    highlight: highlightDirective,
    clickOutside: clickOutsideDirective,
    tooltip: tooltipDirective,
    draggable: draggableDirective,
    permission: permissionDirective,
  },
  data() {
    return {
      isDropdownOpen: false,
      highlightColor: "yellow",
      tooltipText: "Hover me!",
    };
  },
  methods: {
    closeDropdown(): void {
      this.isDropdownOpen = false;
    },
    toggleDropdown(): void {
      this.isDropdownOpen = !this.isDropdownOpen;
    },
    toggleColor(): void {
      this.highlightColor = this.highlightColor === "yellow" ? "cyan" : "yellow";
    },
  },
});
</script>

<template>
  <div class="directives-demo">
    <!-- v-focus directive -->
    <input v-focus type="text" placeholder="Auto-focused" />

    <!-- v-highlight with binding value -->
    <div v-highlight="highlightColor">Highlighted text</div>
    <button @click="toggleColor">Toggle Color</button>

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
