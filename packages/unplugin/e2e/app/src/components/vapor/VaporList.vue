<script setup lang="ts">
import { ref } from 'vue'

const items = ref(['Alpha', 'Beta', 'Gamma'])

function addItem() {
  items.value.push(`Item ${items.value.length + 1}`)
}
</script>

<template vapor>
  <div data-testid="vapor-list">
    <button data-testid="vapor-add" @click="addItem">Add</button>
    <ul data-testid="vapor-items">
      <li v-for="(item, index) in items" :key="index">{{ item }}</li>
    </ul>
  </div>
</template>
