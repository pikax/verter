<script setup lang="ts">
import { ref } from "vue";

const color = ref("red");
const fontSize = ref(16);
const padding = ref(10);
const theme = ref<"light" | "dark">("light");
const borderWidth = ref(2);
</script>

<template>
  <div class="container">
    <h2>CSS v-bind</h2>

    <div class="dynamic-box">
      This box has dynamic styles
    </div>

    <div class="controls">
      <label>
        Color:
        <input v-model="color" type="text" />
      </label>
      <label>
        Font Size:
        <input v-model.number="fontSize" type="range" min="10" max="32" />
        {{ fontSize }}px
      </label>
      <label>
        Padding:
        <input v-model.number="padding" type="range" min="0" max="50" />
        {{ padding }}px
      </label>
      <label>
        Theme:
        <select v-model="theme">
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
      </label>
    </div>
  </div>
</template>

<style scoped>
.container {
  padding: 20px;
}

.dynamic-box {
  /* Direct v-bind with ref */
  color: v-bind(color);
  font-size: v-bind(fontSize + 'px');
  padding: v-bind(padding + 'px');
  border: v-bind(borderWidth + 'px') solid v-bind(color);
  
  /* Conditional based on ref value */
  background-color: v-bind("theme === 'dark' ? '#333' : '#fff'");
}

.controls {
  margin-top: 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.controls label {
  display: flex;
  align-items: center;
  gap: 10px;
}
</style>
