<script setup lang="ts">
import { ref, computed, reactive, watch } from "vue";
import HeaderComp from "./HeaderComp.vue";
import ItemCard from "./ItemCard.vue";
import Pagination from "./Pagination.vue";
import Modal from "./Modal.vue";

const title = ref("Large Component");
const searchQuery = ref("");
const currentPage = ref(1);
const pageSize = ref(20);
const sortBy = ref("name");
const sortOrder = ref<"asc" | "desc">("asc");
const selectedIds = ref<number[]>([]);
const showModal = ref(false);
const modalContent = ref("");

const items = ref(
  Array.from({ length: 100 }, (_, i) => ({
    id: i + 1,
    name: `Item ${i + 1}`,
    description: `Description for item ${i + 1}`,
    category: ["A", "B", "C"][i % 3],
    price: Math.round(Math.random() * 1000) / 10,
    active: i % 4 !== 0,
    tags: [`tag${i % 5}`, `tag${(i + 1) % 5}`],
    createdAt: new Date(Date.now() - i * 86400000).toISOString(),
  })),
);

const categories = computed(() => [...new Set(items.value.map((i) => i.category))]);
const selectedCategory = ref("");

const filteredItems = computed(() => {
  let result = items.value;
  if (searchQuery.value) {
    const q = searchQuery.value.toLowerCase();
    result = result.filter(
      (i) => i.name.toLowerCase().includes(q) || i.description.toLowerCase().includes(q),
    );
  }
  if (selectedCategory.value) {
    result = result.filter((i) => i.category === selectedCategory.value);
  }
  return result;
});

const sortedItems = computed(() => {
  const sorted = [...filteredItems.value];
  sorted.sort((a, b) => {
    const key = sortBy.value as keyof typeof a;
    const av = a[key],
      bv = b[key];
    const cmp = av < bv ? -1 : av > bv ? 1 : 0;
    return sortOrder.value === "asc" ? cmp : -cmp;
  });
  return sorted;
});

const paginatedItems = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value;
  return sortedItems.value.slice(start, start + pageSize.value);
});

const totalPages = computed(() => Math.ceil(sortedItems.value.length / pageSize.value));
const totalPrice = computed(() =>
  filteredItems.value.reduce((sum, i) => sum + i.price, 0).toFixed(2),
);
const activeCount = computed(() => filteredItems.value.filter((i) => i.active).length);
const isAllSelected = computed(() => selectedIds.value.length === paginatedItems.value.length);

function toggleSort(field: string) {
  if (sortBy.value === field) {
    sortOrder.value = sortOrder.value === "asc" ? "desc" : "asc";
  } else {
    sortBy.value = field;
    sortOrder.value = "asc";
  }
}

function toggleSelect(id: number) {
  const idx = selectedIds.value.indexOf(id);
  if (idx >= 0) selectedIds.value.splice(idx, 1);
  else selectedIds.value.push(id);
}

function selectAll() {
  if (isAllSelected.value) selectedIds.value = [];
  else selectedIds.value = paginatedItems.value.map((i) => i.id);
}

function deleteSelected() {
  items.value = items.value.filter((i) => !selectedIds.value.includes(i.id));
  selectedIds.value = [];
}

function openModal(item: any) {
  modalContent.value = JSON.stringify(item, null, 2);
  showModal.value = true;
}

watch(searchQuery, () => {
  currentPage.value = 1;
});
watch(selectedCategory, () => {
  currentPage.value = 1;
});
</script>

<template>
  <div class="large-component">
    <HeaderComp :title="title" :subtitle="`${filteredItems.length} results`" />

    <div class="toolbar">
      <input v-model="searchQuery" type="text" placeholder="Search..." class="search" />
      <select v-model="selectedCategory" class="category-select">
        <option value="">All Categories</option>
        <option v-for="cat of categories" :key="cat" :value="cat">{{ cat }}</option>
      </select>
      <div class="stats">
        <span>Active: {{ activeCount }}</span>
        <span>Total: ${{ totalPrice }}</span>
      </div>
    </div>

    <div v-if="selectedIds.length > 0" class="bulk-actions">
      <span>{{ selectedIds.length }} selected</span>
      <button @click="deleteSelected" class="danger">Delete Selected</button>
    </div>

    <table class="data-table">
      <thead>
        <tr>
          <th><input type="checkbox" :checked="isAllSelected" @change="selectAll" /></th>
          <th @click="toggleSort('name')" :class="{ sorted: sortBy === 'name' }">
            Name {{ sortBy === "name" ? (sortOrder === "asc" ? "↑" : "↓") : "" }}
          </th>
          <th @click="toggleSort('category')">Category</th>
          <th @click="toggleSort('price')">Price</th>
          <th>Tags</th>
          <th>Status</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="item of paginatedItems"
          :key="item.id"
          :class="{ selected: selectedIds.includes(item.id), inactive: !item.active }"
        >
          <td>
            <input
              type="checkbox"
              :checked="selectedIds.includes(item.id)"
              @change="toggleSelect(item.id)"
            />
          </td>
          <td class="name-cell">
            <strong>{{ item.name }}</strong>
            <span class="description">{{ item.description }}</span>
          </td>
          <td>
            <span class="badge" :class="'cat-' + item.category">{{ item.category }}</span>
          </td>
          <td class="price">${{ item.price.toFixed(2) }}</td>
          <td>
            <span v-for="tag of item.tags" :key="tag" class="tag">{{ tag }}</span>
          </td>
          <td>
            <span v-if="item.active" class="status active">Active</span>
            <span v-else class="status inactive">Inactive</span>
          </td>
          <td>
            <button @click="openModal(item)" class="btn-view">View</button>
            <button @click="toggleSelect(item.id)" class="btn-select">
              {{ selectedIds.includes(item.id) ? "Deselect" : "Select" }}
            </button>
          </td>
        </tr>
      </tbody>
    </table>

    <div v-if="paginatedItems.length === 0" class="empty-state">
      <p v-if="searchQuery">No results for "{{ searchQuery }}"</p>
      <p v-else>No items available</p>
    </div>

    <Pagination
      :current-page="currentPage"
      :total-pages="totalPages"
      :page-size="pageSize"
      @update:page="currentPage = $event"
    />

    <Modal v-if="showModal" @close="showModal = false">
      <pre>{{ modalContent }}</pre>
    </Modal>
  </div>
</template>

<style scoped>
.large-component {
  max-width: 1200px;
  margin: 0 auto;
  padding: 20px;
}
.toolbar {
  display: flex;
  gap: 10px;
  margin: 20px 0;
  align-items: center;
}
.search {
  flex: 1;
  padding: 8px;
  border: 1px solid #ddd;
  border-radius: 4px;
}
.category-select {
  padding: 8px;
}
.stats {
  margin-left: auto;
  display: flex;
  gap: 15px;
  color: #666;
}
.bulk-actions {
  padding: 10px;
  background: #fff3cd;
  border-radius: 4px;
  display: flex;
  align-items: center;
  gap: 10px;
}
.data-table {
  width: 100%;
  border-collapse: collapse;
}
.data-table th,
.data-table td {
  padding: 10px;
  border-bottom: 1px solid #eee;
  text-align: left;
}
.data-table th {
  cursor: pointer;
  user-select: none;
}
.data-table th.sorted {
  color: blue;
}
.data-table tr.selected {
  background: #e3f2fd;
}
.data-table tr.inactive {
  opacity: 0.6;
}
.name-cell {
  display: flex;
  flex-direction: column;
}
.description {
  font-size: 0.85em;
  color: #888;
}
.badge {
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 0.85em;
}
.cat-A {
  background: #e8f5e9;
}
.cat-B {
  background: #e3f2fd;
}
.cat-C {
  background: #fff3e0;
}
.price {
  font-family: monospace;
}
.tag {
  margin: 0 2px;
  padding: 1px 6px;
  background: #f0f0f0;
  border-radius: 3px;
  font-size: 0.8em;
}
.status {
  padding: 2px 8px;
  border-radius: 3px;
  font-size: 0.85em;
}
.status.active {
  background: #c8e6c9;
  color: #2e7d32;
}
.status.inactive {
  background: #ffcdd2;
  color: #c62828;
}
.danger {
  color: white;
  background: #e53935;
  border: none;
  padding: 4px 12px;
  border-radius: 3px;
}
.empty-state {
  padding: 40px;
  text-align: center;
  color: #999;
}
</style>
