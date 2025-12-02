<script setup>
// Function directive - simple form
const vFocus = (el) => {
  el.focus();
};

// Object directive with all hooks
const vHighlight = {
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
const vClickOutside = {
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

// Directive with argument
const vTooltip = {
  mounted(el, binding) {
    const position = binding.arg || "top";
    const text = binding.value;
    el.setAttribute("data-tooltip", text);
    el.setAttribute("data-tooltip-position", position);
  },
  updated(el, binding) {
    el.setAttribute("data-tooltip", binding.value);
  },
};

// Directive using modifiers
const vDraggable = {
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

// Directive with permission check
const vPermission = {
  mounted(el, binding) {
    const { role, action } = binding.value;
    const hasPermission = checkPermission(role, action);
    if (!hasPermission) {
      el.style.display = "none";
    }
  },
};

function checkPermission(role, action) {
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
