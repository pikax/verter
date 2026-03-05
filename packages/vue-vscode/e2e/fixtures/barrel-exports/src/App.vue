<script setup lang="ts">
import { ref, computed } from 'vue'
import { Overlay, Button } from './components'

const count = ref(0)
const doubled = computed(() => count.value * 2)
const showOverlay = ref(false)

function increment() { count.value++ }
</script>
<template>
  <div>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <button @click="increment">+</button>
    <Button label="Open" @click="showOverlay = true" />
    <Overlay :show="showOverlay" :zIndex="100" :lockScroll="true">
      <p>Overlay content</p>
    </Overlay>
  </div>
</template>
