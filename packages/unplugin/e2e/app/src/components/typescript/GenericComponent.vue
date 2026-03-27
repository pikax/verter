<script setup lang="ts" generic="T extends { id: number; label: string }">
import { ref } from "vue";

const props = defineProps<{
  items: T[];
}>();

const currentIndex = ref(0);

function next() {
  currentIndex.value = (currentIndex.value + 1) % props.items.length;
}

const currentItem = () => props.items[currentIndex.value];
</script>

<template>
  <div data-testid="generic-component">
    <span data-testid="generic-current">{{ currentItem()?.label }}</span>
    <span data-testid="generic-index">{{ currentIndex }}</span>
    <button data-testid="generic-next" @click="next">Next</button>
  </div>
</template>
