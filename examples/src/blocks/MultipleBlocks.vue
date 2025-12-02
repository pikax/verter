<script setup lang="ts">
// Multiple script blocks and style blocks patterns

import { ref } from "vue";

const count = ref(0);
const theme = ref<"light" | "dark">("light");
</script>

<script lang="ts">
// Regular script block for component options
export default {
  name: "MultiBlockComponent",
  inheritAttrs: false,
};
</script>

<template>
  <div :class="['container', theme]">
    <h2>Multiple Blocks</h2>
    <p>Count: {{ count }}</p>
    <button @click="count++">Increment</button>
    <button @click="theme = theme === 'light' ? 'dark' : 'light'">
      Toggle Theme
    </button>
  </div>
</template>

<!-- Plain CSS -->
<style>
.container {
  padding: 20px;
}
</style>

<!-- Scoped CSS -->
<style scoped>
h2 {
  color: navy;
}

p {
  font-size: 16px;
}
</style>

<!-- SCSS -->
<style lang="scss" scoped>
.container {
  &.light {
    background: white;
    color: black;
  }

  &.dark {
    background: #333;
    color: white;
  }

  button {
    margin: 5px;
    padding: 8px 16px;
    
    &:hover {
      opacity: 0.8;
    }
  }
}
</style>

<!-- LESS -->
<style lang="less" scoped>
@primary: blue;
@secondary: green;

.container {
  border: 1px solid @primary;

  h2 {
    border-bottom: 2px solid @secondary;
  }
}
</style>

<!-- CSS Module -->
<style module>
.moduleClass {
  font-weight: bold;
}
</style>

<!-- Named CSS Module -->
<style module="custom">
.customClass {
  text-decoration: underline;
}
</style>
