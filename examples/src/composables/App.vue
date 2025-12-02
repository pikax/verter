<script setup lang="ts">
import { useMouse, useCounter, useToggle } from "./composables";

// Using composables
const { x, y } = useMouse();
const { count, increment, decrement, reset } = useCounter({
  initialValue: 10,
  min: 0,
  max: 100,
});
const { state: isVisible, toggle: toggleVisibility } = useToggle(true);
</script>

<template>
  <div>
    <h2>Composables Example</h2>

    <section>
      <h3>Mouse Position</h3>
      <p>X: {{ x }}, Y: {{ y }}</p>
    </section>

    <section>
      <h3>Counter (min: 0, max: 100)</h3>
      <p>Count: {{ count }}</p>
      <button @click="decrement">-</button>
      <button @click="increment">+</button>
      <button @click="reset">Reset</button>
    </section>

    <section>
      <h3>Toggle</h3>
      <p>Visible: {{ isVisible }}</p>
      <button @click="toggleVisibility">Toggle</button>
      <div v-if="isVisible">This content is visible!</div>
    </section>
  </div>
</template>
