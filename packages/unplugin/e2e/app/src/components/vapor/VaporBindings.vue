<script setup lang="ts">
import { ref, computed } from "vue";

const color = ref("blue");
const fontSize = ref(16);
const styleObj = computed(() => ({
  color: color.value,
  fontSize: fontSize.value + "px",
}));

function toggleColor() {
  color.value = color.value === "blue" ? "red" : "blue";
}

function bigger() {
  fontSize.value += 2;
}
</script>

<template vapor>
  <div data-testid="vapor-bindings">
    <span data-testid="vapor-styled" :style="styleObj">Styled text</span>
    <button data-testid="vapor-color" @click="toggleColor">Toggle Color</button>
    <button data-testid="vapor-bigger" @click="bigger">Bigger</button>
    <span data-testid="vapor-color-val">{{ color }}</span>
    <span data-testid="vapor-size-val">{{ fontSize }}</span>
  </div>
</template>
