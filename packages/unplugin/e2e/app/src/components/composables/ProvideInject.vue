<script setup lang="ts">
import { ref, provide } from 'vue'
import InjectedChild from './InjectedChild.vue'

const theme = ref('light')

provide('theme', theme)

function toggleTheme() {
  theme.value = theme.value === 'light' ? 'dark' : 'light'
}
</script>

<template>
  <div data-testid="provide-inject">
    <span data-testid="provider-value">{{ theme }}</span>
    <button data-testid="toggle-theme" @click="toggleTheme">Toggle Theme</button>
    <InjectedChild />
  </div>
</template>
