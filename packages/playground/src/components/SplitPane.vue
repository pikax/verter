<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";

const props = defineProps<{
  direction?: "horizontal" | "vertical";
  initialSplit?: number;
}>();

const split = ref(props.initialSplit ?? 50);
const isDragging = ref(false);
const container = ref<HTMLElement>();

function startDrag(e: MouseEvent) {
  e.preventDefault();
  isDragging.value = true;
  document.addEventListener("mousemove", onDrag);
  document.addEventListener("mouseup", stopDrag);
}

function onDrag(e: MouseEvent) {
  if (!isDragging.value || !container.value) return;

  const rect = container.value.getBoundingClientRect();
  if (props.direction === "vertical") {
    const newSplit = ((e.clientY - rect.top) / rect.height) * 100;
    split.value = Math.max(20, Math.min(80, newSplit));
  } else {
    const newSplit = ((e.clientX - rect.left) / rect.width) * 100;
    split.value = Math.max(20, Math.min(80, newSplit));
  }
}

function stopDrag() {
  isDragging.value = false;
  document.removeEventListener("mousemove", onDrag);
  document.removeEventListener("mouseup", stopDrag);
}

onUnmounted(() => {
  document.removeEventListener("mousemove", onDrag);
  document.removeEventListener("mouseup", stopDrag);
});
</script>

<template>
  <div
    ref="container"
    class="split-pane"
    :class="[direction ?? 'horizontal', { dragging: isDragging }]"
  >
    <div class="pane first" :style="{ flexBasis: split + '%' }">
      <slot name="first" />
    </div>
    <div class="divider" @mousedown="startDrag" />
    <div class="pane second" :style="{ flexBasis: 100 - split + '%' }">
      <slot name="second" />
    </div>
  </div>
</template>

<style scoped>
.split-pane {
  display: flex;
  height: 100%;
  width: 100%;
  overflow: hidden;
}

.split-pane.horizontal {
  flex-direction: row;
}

.split-pane.vertical {
  flex-direction: column;
}

.pane {
  overflow: hidden;
  min-width: 0;
  min-height: 0;
}

.divider {
  flex-shrink: 0;
  background: var(--border-color);
  transition: background 0.2s;
}

.horizontal > .divider {
  width: 4px;
  cursor: col-resize;
}

.vertical > .divider {
  height: 4px;
  cursor: row-resize;
}

.divider:hover,
.dragging .divider {
  background: var(--accent-color);
}

.dragging {
  user-select: none;
}
</style>
