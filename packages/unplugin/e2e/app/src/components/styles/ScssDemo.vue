<script setup lang="ts">
import { ref } from 'vue'
const darkTheme = ref(false)
</script>

<template>
  <div data-testid="scss-demo" :class="{ dark: darkTheme }">
    <p data-testid="scss-text" class="scss-styled">SCSS Styled Text</p>
    <button data-testid="scss-toggle" @click="darkTheme = !darkTheme">Toggle Theme</button>
  </div>
</template>

<style lang="scss" scoped>
$primary: rgb(0, 100, 200);
$dark-bg: rgb(30, 30, 30);

.scss-styled {
  color: $primary;
  font-size: 16px;
}

.dark {
  background-color: $dark-bg;

  .scss-styled {
    color: rgb(200, 200, 255);
  }
}
</style>
