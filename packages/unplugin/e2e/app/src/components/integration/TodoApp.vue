<script setup lang="ts">
import { ref, computed } from 'vue'

interface Todo {
  id: number
  text: string
  completed: boolean
}

const newTodoText = ref('')
const todos = ref<Todo[]>([])
const filter = ref<'all' | 'active' | 'completed'>('all')
let nextId = 1

const filteredTodos = computed(() => {
  switch (filter.value) {
    case 'active': return todos.value.filter(t => !t.completed)
    case 'completed': return todos.value.filter(t => t.completed)
    default: return todos.value
  }
})

const remaining = computed(() => todos.value.filter(t => !t.completed).length)

function addTodo() {
  const text = newTodoText.value.trim()
  if (!text) return
  todos.value.push({ id: nextId++, text, completed: false })
  newTodoText.value = ''
}

function toggleTodo(id: number) {
  const todo = todos.value.find(t => t.id === id)
  if (todo) todo.completed = !todo.completed
}

function deleteTodo(id: number) {
  todos.value = todos.value.filter(t => t.id !== id)
}

function setFilter(f: 'all' | 'active' | 'completed') {
  filter.value = f
}
</script>

<template>
  <div data-testid="todo-app">
    <div data-testid="todo-input-area">
      <input data-testid="todo-input" v-model="newTodoText" @keyup.enter="addTodo" placeholder="Add todo..." />
      <button data-testid="todo-add" @click="addTodo">Add</button>
    </div>

    <ul data-testid="todo-list">
      <li v-for="todo in filteredTodos" :key="todo.id" data-testid="todo-item" :data-completed="todo.completed">
        <input type="checkbox" :checked="todo.completed" @change="toggleTodo(todo.id)" data-testid="todo-checkbox" />
        <span data-testid="todo-text" :class="{ completed: todo.completed }">{{ todo.text }}</span>
        <button data-testid="todo-delete" @click="deleteTodo(todo.id)">Delete</button>
      </li>
    </ul>

    <div data-testid="todo-filters">
      <button data-testid="filter-all" @click="setFilter('all')" :class="{ active: filter === 'all' }">All</button>
      <button data-testid="filter-active" @click="setFilter('active')" :class="{ active: filter === 'active' }">Active</button>
      <button data-testid="filter-completed" @click="setFilter('completed')" :class="{ active: filter === 'completed' }">Completed</button>
    </div>

    <span data-testid="todo-remaining">{{ remaining }} remaining</span>
  </div>
</template>

<style scoped>
.completed {
  text-decoration: line-through;
  opacity: 0.6;
}
.active {
  font-weight: bold;
}
</style>
