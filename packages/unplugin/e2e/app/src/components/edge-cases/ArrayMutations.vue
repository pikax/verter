<script setup lang="ts">
import { ref } from "vue";

const items = ref([3, 1, 4, 1, 5]);

function pushItem() {
  items.value.push(items.value.length + 1);
}
function popItem() {
  items.value.pop();
}
function spliceItem() {
  items.value.splice(1, 1);
}
function sortItems() {
  items.value.sort((a, b) => a - b);
}
function reverseItems() {
  items.value.reverse();
}
</script>

<template>
  <div data-testid="array-mutations">
    <ul data-testid="array-list">
      <li v-for="(item, i) in items" :key="i" data-testid="array-item">{{ item }}</li>
    </ul>
    <button data-testid="array-push" @click="pushItem">Push</button>
    <button data-testid="array-pop" @click="popItem">Pop</button>
    <button data-testid="array-splice" @click="spliceItem">Splice</button>
    <button data-testid="array-sort" @click="sortItems">Sort</button>
    <button data-testid="array-reverse" @click="reverseItems">Reverse</button>
  </div>
</template>
