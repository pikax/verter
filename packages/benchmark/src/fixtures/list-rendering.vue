<script setup lang="ts">
import { ref } from "vue";

interface Item {
  id: number;
  text: string;
  done: boolean;
}

const items = ref<Item[]>([
  { id: 1, text: "Learn Vue", done: true },
  { id: 2, text: "Build something", done: false },
  { id: 3, text: "Ship it", done: false },
]);

const newItemText = ref("");

function addItem() {
  if (newItemText.value.trim()) {
    items.value.push({
      id: Date.now(),
      text: newItemText.value,
      done: false,
    });
    newItemText.value = "";
  }
}

function toggleItem(id: number) {
  const item = items.value.find((i) => i.id === id);
  if (item) item.done = !item.done;
}

function removeItem(id: number) {
  items.value = items.value.filter((i) => i.id !== id);
}
</script>

<template>
  <div>
    <h2>Todo List</h2>
    <ul>
      <li v-for="item in items" :key="item.id">
        <input type="checkbox" :checked="item.done" @change="toggleItem(item.id)" />
        <span :class="{ done: item.done }">{{ item.text }}</span>
        <button @click="removeItem(item.id)">Remove</button>
      </li>
    </ul>
    <div>
      <input v-model="newItemText" @keyup.enter="addItem" placeholder="Add new item" />
      <button @click="addItem">Add</button>
    </div>
  </div>
</template>

<style scoped>
.done {
  text-decoration: line-through;
  color: #888;
}
</style>
