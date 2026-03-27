<script setup lang="ts">
import { ref } from "vue";

const condition = ref<"a" | "b" | "c">("a");
const visible = ref(true);

function cycleCondition() {
  const order: Array<"a" | "b" | "c"> = ["a", "b", "c"];
  const idx = order.indexOf(condition.value);
  condition.value = order[(idx + 1) % order.length];
}

function toggleVisible() {
  visible.value = !visible.value;
}
</script>

<template>
  <div data-testid="conditional-rendering">
    <div v-if="condition === 'a'" data-testid="cond-a">Condition A</div>
    <div v-else-if="condition === 'b'" data-testid="cond-b">Condition B</div>
    <div v-else data-testid="cond-c">Condition C</div>
    <button data-testid="cycle-condition" @click="cycleCondition">Cycle</button>

    <div v-show="visible" data-testid="v-show-target">Visible Content</div>
    <button data-testid="toggle-visible" @click="toggleVisible">Toggle Show</button>
  </div>
</template>
