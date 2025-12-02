<script>
import { defineComponent } from "vue";

// Define directives outside component
const focusDirective = {
  mounted(el) {
    el.focus();
  },
};

const highlightDirective = {
  mounted(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
  updated(el, binding) {
    el.style.backgroundColor = binding.value || "yellow";
  },
};

const clickOutsideDirective = {
  mounted(el, binding) {
    const handler = (event) => {
      const target = event.target;
      const excluded = binding.value.exclude || [];
      const isExcluded = excluded.some((selector) =>
        target.closest(selector)
      );
      if (!el.contains(target) && !isExcluded) {
        binding.value.handler(event);
      }
    };
    document.addEventListener("click", handler);
    el.__clickOutsideHandler = handler;
  },
  unmounted(el) {
    document.removeEventListener("click", el.__clickOutsideHandler);
  },
};

const tooltipDirective = {
  mounted(el, binding) {
    const position = binding.arg || "top";
    el.setAttribute("data-tooltip", binding.value);
    el.setAttribute("data-tooltip-position", position);
  },
  updated(el, binding) {
    el.setAttribute("data-tooltip", binding.value);
  },
};

const draggableDirective = {
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

function checkPermission(role, action) {
  return true;
}

const permissionDirective = {
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
    closeDropdown() {
      this.isDropdownOpen = false;
    },
    toggleDropdown() {
      this.isDropdownOpen = !this.isDropdownOpen;
    },
    toggleColor() {
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
