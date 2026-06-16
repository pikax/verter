<template>
  <Teleport to="body">
    <Transition name="drawer">
      <!-- @dx-anchor drawer.overlay -->
      <div v-if="open" class="drawer-overlay" @click="close" @keydown.esc="close">
        <!-- @dx-anchor drawer.panel -->
        <aside class="drawer-panel" @click.stop="() => {}">
          <DrawerHeader :title="title" />
          <div class="drawer-body"><!-- @dx-anchor drawer.slot --><slot :width="width" /></div>
        </aside>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";

import DrawerHeader from "./DrawerHeader.vue";

const props = withDefaults(defineProps<{ title?: string; side?: "left" | "right" }>(), {
  title: "Panel",
  side: "right",
});

const emit = defineEmits<{ close: []; opened: [side: string] }>(); // @dx-anchor drawer.emit

const open = defineModel<boolean>({ default: false }); // @dx-anchor drawer.model

const width = ref(320);

// @dx-anchor drawer.editPoint

const title = computed(() => props.title); // @dx-anchor drawer.computed

function close() {
  open.value = false;
  emit("close");
}
</script>
