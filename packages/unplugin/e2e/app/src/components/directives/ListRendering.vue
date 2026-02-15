<script setup lang="ts">
import { ref } from 'vue'

const items = ref(['Apple', 'Banana', 'Cherry'])
let counter = 0

function addItem() {
  items.value.push(`Item-${++counter}`)
}

function removeItem() {
  items.value.splice(0, 1)
}

function sortItems() {
  items.value.sort()
}

function reverseItems() {
  items.value.reverse()
}
</script>

<template>
  <div data-testid="list-rendering">
    <ul data-testid="list-items">
      <li v-for="(item, i) in items" :key="i" data-testid="list-item">{{ item }}</li>
    </ul>
    <button data-testid="list-add" @click="addItem">Add</button>
    <button data-testid="list-remove" @click="removeItem">Remove</button>
    <button data-testid="list-sort" @click="sortItems">Sort</button>
    <button data-testid="list-reverse" @click="reverseItems">Reverse</button>
  </div>
</template>
