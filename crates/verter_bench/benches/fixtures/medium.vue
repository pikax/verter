<script setup lang="ts">
import { ref, computed, reactive } from "vue";
import MyComponent from "./MyComponent.vue";

const title = ref("Dashboard");
const items = ref([
  { id: 1, name: "Item 1", active: true },
  { id: 2, name: "Item 2", active: false },
  { id: 3, name: "Item 3", active: true },
]);
const filter = ref("");
const count = computed(() => items.value.length);
const state = reactive({ loading: false, error: null });

const filteredItems = computed(() =>
  items.value.filter((item) => item.name.toLowerCase().includes(filter.value.toLowerCase())),
);

function addItem() {
  items.value.push({ id: Date.now(), name: `Item ${count.value + 1}`, active: true });
}

function removeItem(id: number) {
  items.value = items.value.filter((item) => item.id !== id);
}

function toggleItem(id: number) {
  const item = items.value.find((i) => i.id === id);
  if (item) item.active = !item.active;
}
</script>

<template>
  <div class="dashboard">
    <header class="header">
      <h1>{{ title }}</h1>
      <span class="count">{{ count }} items</span>
    </header>

    <div v-if="state.loading" class="loading">Loading...</div>
    <div v-else-if="state.error" class="error">{{ state.error }}</div>
    <div v-else class="content">
      <input v-model="filter" type="text" placeholder="Filter items..." class="filter-input" />

      <ul class="item-list">
        <li
          v-for="item of filteredItems"
          :key="item.id"
          :class="{ active: item.active, inactive: !item.active }"
          class="item"
        >
          <span class="item-name">{{ item.name }}</span>
          <div class="item-actions">
            <button @click="toggleItem(item.id)" :disabled="state.loading">
              {{ item.active ? "Deactivate" : "Activate" }}
            </button>
            <button @click="removeItem(item.id)" class="danger">Remove</button>
          </div>
        </li>
      </ul>

      <div class="footer">
        <button @click="addItem" class="primary">Add Item</button>
        <MyComponent :items="items" :count="count" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.dashboard {
  max-width: 800px;
  margin: 0 auto;
  padding: 20px;
}
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.filter-input {
  width: 100%;
  padding: 8px;
  margin: 10px 0;
}
.item-list {
  list-style: none;
  padding: 0;
}
.item {
  display: flex;
  justify-content: space-between;
  padding: 10px;
  border-bottom: 1px solid #eee;
}
.item.active {
  background: #e8f5e9;
}
.item.inactive {
  opacity: 0.5;
}
.danger {
  color: red;
}
.primary {
  color: blue;
}
</style>
