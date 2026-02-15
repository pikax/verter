<script setup lang="ts">
import { ref, computed, watch, onMounted, provide, watchEffect } from 'vue'

// Types
interface User {
  id: number
  name: string
  email: string
  role: 'admin' | 'editor' | 'viewer'
  avatar?: string
  status: 'online' | 'offline' | 'away'
}

interface Task {
  id: string
  title: string
  description: string
  assignee: number
  priority: 'low' | 'medium' | 'high' | 'critical'
  status: 'todo' | 'in-progress' | 'review' | 'done'
  dueDate: Date
  tags: string[]
  comments: Comment[]
  attachments: Attachment[]
}

interface Comment {
  id: string
  userId: number
  text: string
  timestamp: Date
}

interface Attachment {
  id: string
  name: string
  url: string
  size: number
}

interface Filter {
  status: string[]
  priority: string[]
  assignee: number[]
  tags: string[]
  search: string
}

// Props & Emits
const props = withDefaults(defineProps<{
  projectId?: number
  initialView?: 'list' | 'board' | 'calendar'
}>(), {
  initialView: 'list'
})

const emit = defineEmits<{
  taskCreated: [task: Task]
  taskUpdated: [task: Task]
  taskDeleted: [taskId: string]
  filterChanged: [filter: Filter]
}>()

// State
const users = ref<User[]>([])
const tasks = ref<Task[]>([])
const selectedTask = ref<Task | null>(null)
const currentView = ref(props.initialView)
const isLoading = ref(false)
const error = ref<string | null>(null)
const showTaskModal = ref(false)
const showFilterPanel = ref(false)

const filter = ref<Filter>({
  status: [],
  priority: [],
  assignee: [],
  tags: [],
  search: ''
})

const newTask = ref<Partial<Task>>({
  title: '',
  description: '',
  priority: 'medium',
  status: 'todo',
  tags: [],
  comments: [],
  attachments: []
})

const sortBy = ref<'dueDate' | 'priority' | 'status' | 'title'>('dueDate')
const sortOrder = ref<'asc' | 'desc'>('asc')
const groupBy = ref<'status' | 'priority' | 'assignee' | 'none'>('status')

// Computed
const filteredTasks = computed(() => {
  let result = tasks.value

  if (filter.value.search) {
    const search = filter.value.search.toLowerCase()
    result = result.filter(task =>
      task.title.toLowerCase().includes(search) ||
      task.description.toLowerCase().includes(search)
    )
  }

  if (filter.value.status.length > 0) {
    result = result.filter(task => filter.value.status.includes(task.status))
  }

  if (filter.value.priority.length > 0) {
    result = result.filter(task => filter.value.priority.includes(task.priority))
  }

  if (filter.value.assignee.length > 0) {
    result = result.filter(task => filter.value.assignee.includes(task.assignee))
  }

  if (filter.value.tags.length > 0) {
    result = result.filter(task =>
      filter.value.tags.some(tag => task.tags.includes(tag))
    )
  }

  return result
})

const sortedTasks = computed(() => {
  const sorted = [...filteredTasks.value]
  
  sorted.sort((a, b) => {
    let compareValue = 0
    
    switch (sortBy.value) {
      case 'title':
        compareValue = a.title.localeCompare(b.title)
        break
      case 'dueDate':
        compareValue = a.dueDate.getTime() - b.dueDate.getTime()
        break
      case 'priority':
        const priorityOrder = { low: 0, medium: 1, high: 2, critical: 3 }
        compareValue = priorityOrder[a.priority] - priorityOrder[b.priority]
        break
      case 'status':
        const statusOrder = { todo: 0, 'in-progress': 1, review: 2, done: 3 }
        compareValue = statusOrder[a.status] - statusOrder[b.status]
        break
    }
    
    return sortOrder.value === 'asc' ? compareValue : -compareValue
  })
  
  return sorted
})

const groupedTasks = computed(() => {
  if (groupBy.value === 'none') {
    return { 'All Tasks': sortedTasks.value }
  }

  const groups: Record<string, Task[]> = {}
  
  sortedTasks.value.forEach(task => {
    let key: string
    
    switch (groupBy.value) {
      case 'status':
        key = task.status
        break
      case 'priority':
        key = task.priority
        break
      case 'assignee':
        const user = users.value.find(u => u.id === task.assignee)
        key = user?.name || 'Unassigned'
        break
      default:
        key = 'Other'
    }
    
    if (!groups[key]) {
      groups[key] = []
    }
    groups[key].push(task)
  })
  
  return groups
})

const stats = computed(() => ({
  total: tasks.value.length,
  todo: tasks.value.filter(t => t.status === 'todo').length,
  inProgress: tasks.value.filter(t => t.status === 'in-progress').length,
  review: tasks.value.filter(t => t.status === 'review').length,
  done: tasks.value.filter(t => t.status === 'done').length,
  overdue: tasks.value.filter(t => 
    t.status !== 'done' && t.dueDate < new Date()
  ).length,
  critical: tasks.value.filter(t => t.priority === 'critical').length
}))

const allTags = computed(() => {
  const tagSet = new Set<string>()
  tasks.value.forEach(task => {
    task.tags.forEach(tag => tagSet.add(tag))
  })
  return Array.from(tagSet).sort()
})

const hasActiveFilters = computed(() => {
  return filter.value.status.length > 0 ||
         filter.value.priority.length > 0 ||
         filter.value.assignee.length > 0 ||
         filter.value.tags.length > 0 ||
         filter.value.search.length > 0
})

// Methods
async function loadTasks() {
  isLoading.value = true
  error.value = null
  
  try {
    // Simulate API call
    await new Promise(resolve => setTimeout(resolve, 800))
    
    tasks.value = [
      {
        id: '1',
        title: 'Implement authentication',
        description: 'Add JWT-based authentication',
        assignee: 1,
        priority: 'high',
        status: 'in-progress',
        dueDate: new Date('2026-02-20'),
        tags: ['backend', 'security'],
        comments: [],
        attachments: []
      },
      {
        id: '2',
        title: 'Design dashboard UI',
        description: 'Create mockups for admin dashboard',
        assignee: 2,
        priority: 'medium',
        status: 'todo',
        dueDate: new Date('2026-02-25'),
        tags: ['frontend', 'design'],
        comments: [],
        attachments: []
      }
    ]
    
    users.value = [
      { id: 1, name: 'Alice', email: 'alice@example.com', role: 'admin', status: 'online' },
      { id: 2, name: 'Bob', email: 'bob@example.com', role: 'editor', status: 'online' }
    ]
  } catch (e) {
    error.value = 'Failed to load tasks'
  } finally {
    isLoading.value = false
  }
}

function createTask() {
  if (!newTask.value.title) return
  
  const task: Task = {
    id: Date.now().toString(),
    title: newTask.value.title,
    description: newTask.value.description || '',
    assignee: newTask.value.assignee || users.value[0]?.id,
    priority: newTask.value.priority || 'medium',
    status: newTask.value.status || 'todo',
    dueDate: newTask.value.dueDate || new Date(),
    tags: newTask.value.tags || [],
    comments: [],
    attachments: []
  }
  
  tasks.value.push(task)
  emit('taskCreated', task)
  
  showTaskModal.value = false
  resetNewTask()
}

function updateTask(task: Task) {
  const index = tasks.value.findIndex(t => t.id === task.id)
  if (index !== -1) {
    tasks.value[index] = task
    emit('taskUpdated', task)
  }
}

function deleteTask(taskId: string) {
  tasks.value = tasks.value.filter(t => t.id !== taskId)
  emit('taskDeleted', taskId)
  if (selectedTask.value?.id === taskId) {
    selectedTask.value = null
  }
}

function selectTask(task: Task) {
  selectedTask.value = task
}

function resetNewTask() {
  newTask.value = {
    title: '',
    description: '',
    priority: 'medium',
    status: 'todo',
    tags: [],
    comments: [],
    attachments: []
  }
}

function clearFilters() {
  filter.value = {
    status: [],
    priority: [],
    assignee: [],
    tags: [],
    search: ''
  }
}

function toggleFilterValue(filterType: keyof Filter, value: any) {
  const filterArray = filter.value[filterType] as any[]
  const index = filterArray.indexOf(value)
  
  if (index === -1) {
    filterArray.push(value)
  } else {
    filterArray.splice(index, 1)
  }
}

function getUserById(id: number) {
  return users.value.find(u => u.id === id)
}

function formatDate(date: Date) {
  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric'
  }).format(date)
}

function isOverdue(task: Task) {
  return task.status !== 'done' && task.dueDate < new Date()
}

function getPriorityColor(priority: Task['priority']) {
  const colors = {
    low: '#4CAF50',
    medium: '#2196F3',
    high: '#FF9800',
    critical: '#F44336'
  }
  return colors[priority]
}

function getStatusColor(status: Task['status']) {
  const colors = {
    todo: '#9E9E9E',
    'in-progress': '#2196F3',
    review: '#FF9800',
    done: '#4CAF50'
  }
  return colors[status]
}

// Watchers
watch(filter, (newFilter) => {
  emit('filterChanged', newFilter)
}, { deep: true })

watch(() => props.projectId, () => {
  loadTasks()
})

watchEffect(() => {
  if (hasActiveFilters.value) {
    console.log('Active filters:', filter.value)
  }
})

// Lifecycle
onMounted(() => {
  loadTasks()
})

// Provide
provide('users', users)
provide('updateTask', updateTask)
provide('deleteTask', deleteTask)
</script>

<template>
  <div class="task-manager">
    <!-- Header -->
    <header class="header">
      <div class="header-left">
        <h1>Task Manager</h1>
        <div class="stats-pills">
          <span class="pill">{{ stats.total }} Total</span>
          <span class="pill todo">{{ stats.todo }} To Do</span>
          <span class="pill in-progress">{{ stats.inProgress }} In Progress</span>
          <span class="pill done">{{ stats.done }} Done</span>
          <span v-if="stats.overdue > 0" class="pill overdue">{{ stats.overdue }} Overdue</span>
        </div>
      </div>
      <div class="header-actions">
        <button @click="showFilterPanel = !showFilterPanel" class="btn-secondary">
          <span>🔍</span> Filters
          <span v-if="hasActiveFilters" class="badge">{{ filter.status.length + filter.priority.length + filter.assignee.length }}</span>
        </button>
        <button @click="showTaskModal = true" class="btn-primary">
          <span>+</span> New Task
        </button>
      </div>
    </header>

    <!-- Filters Panel -->
    <aside v-if="showFilterPanel" class="filter-panel">
      <div class="filter-section">
        <h3>Search</h3>
        <input 
          v-model="filter.search" 
          type="text" 
          placeholder="Search tasks..."
          class="search-input"
        >
      </div>

      <div class="filter-section">
        <h3>Status</h3>
        <label v-for="status in ['todo', 'in-progress', 'review', 'done']" :key="status">
          <input 
            type="checkbox" 
            :checked="filter.status.includes(status)"
            @change="toggleFilterValue('status', status)"
          >
          {{ status }}
        </label>
      </div>

      <div class="filter-section">
        <h3>Priority</h3>
        <label v-for="priority in ['low', 'medium', 'high', 'critical']" :key="priority">
          <input 
            type="checkbox" 
            :checked="filter.priority.includes(priority)"
            @change="toggleFilterValue('priority', priority)"
          >
          {{ priority }}
        </label>
      </div>

      <div class="filter-section">
        <h3>Assignee</h3>
        <label v-for="user in users" :key="user.id">
          <input 
            type="checkbox" 
            :checked="filter.assignee.includes(user.id)"
            @change="toggleFilterValue('assignee', user.id)"
          >
          {{ user.name }}
        </label>
      </div>

      <div class="filter-section">
        <h3>Tags</h3>
        <label v-for="tag in allTags" :key="tag">
          <input 
            type="checkbox" 
            :checked="filter.tags.includes(tag)"
            @change="toggleFilterValue('tags', tag)"
          >
          {{ tag }}
        </label>
      </div>

      <button v-if="hasActiveFilters" @click="clearFilters" class="btn-secondary">
        Clear All Filters
      </button>
    </aside>

    <!-- Main Content -->
    <main class="main-content">
      <!-- Toolbar -->
      <div class="toolbar">
        <div class="view-switcher">
          <button 
            :class="{ active: currentView === 'list' }"
            @click="currentView = 'list'"
          >
            List
          </button>
          <button 
            :class="{ active: currentView === 'board' }"
            @click="currentView = 'board'"
          >
            Board
          </button>
          <button 
            :class="{ active: currentView === 'calendar' }"
            @click="currentView = 'calendar'"
          >
            Calendar
          </button>
        </div>

        <div class="sort-controls">
          <label>
            Sort by:
            <select v-model="sortBy">
              <option value="dueDate">Due Date</option>
              <option value="priority">Priority</option>
              <option value="status">Status</option>
              <option value="title">Title</option>
            </select>
          </label>
          <button @click="sortOrder = sortOrder === 'asc' ? 'desc' : 'asc'">
            {{ sortOrder === 'asc' ? '↑' : '↓' }}
          </button>
        </div>

        <div class="group-controls">
          <label>
            Group by:
            <select v-model="groupBy">
              <option value="none">None</option>
              <option value="status">Status</option>
              <option value="priority">Priority</option>
              <option value="assignee">Assignee</option>
            </select>
          </label>
        </div>
      </div>

      <!-- Loading State -->
      <div v-if="isLoading" class="loading">
        <div class="spinner"></div>
        <p>Loading tasks...</p>
      </div>

      <!-- Error State -->
      <div v-else-if="error" class="error-state">
        <p>{{ error }}</p>
        <button @click="loadTasks" class="btn-primary">Retry</button>
      </div>

      <!-- Empty State -->
      <div v-else-if="sortedTasks.length === 0" class="empty-state">
        <p v-if="hasActiveFilters">No tasks match your filters</p>
        <p v-else>No tasks yet. Create your first task!</p>
        <button v-if="hasActiveFilters" @click="clearFilters" class="btn-secondary">
          Clear Filters
        </button>
        <button v-else @click="showTaskModal = true" class="btn-primary">
          Create Task
        </button>
      </div>

      <!-- List View -->
      <div v-else-if="currentView === 'list'" class="list-view">
        <div v-for="(groupTasks, groupName) in groupedTasks" :key="groupName" class="task-group">
          <h2 class="group-header">{{ groupName }} ({{ groupTasks.length }})</h2>
          <div class="task-list">
            <div 
              v-for="task in groupTasks" 
              :key="task.id"
              class="task-item"
              :class="{ 
                selected: selectedTask?.id === task.id,
                overdue: isOverdue(task)
              }"
              @click="selectTask(task)"
            >
              <div class="task-header">
                <h3>{{ task.title }}</h3>
                <div class="task-badges">
                  <span 
                    class="badge priority"
                    :style="{ backgroundColor: getPriorityColor(task.priority) }"
                  >
                    {{ task.priority }}
                  </span>
                  <span 
                    class="badge status"
                    :style="{ backgroundColor: getStatusColor(task.status) }"
                  >
                    {{ task.status }}
                  </span>
                </div>
              </div>
              
              <p class="task-description">{{ task.description }}</p>
              
              <div class="task-meta">
                <div class="assignee">
                  <span class="avatar">{{ getUserById(task.assignee)?.name.charAt(0) }}</span>
                  <span>{{ getUserById(task.assignee)?.name }}</span>
                </div>
                <div class="due-date" :class="{ overdue: isOverdue(task) }">
                  📅 {{ formatDate(task.dueDate) }}
                </div>
                <div v-if="task.tags.length > 0" class="tags">
                  <span v-for="tag in task.tags" :key="tag" class="tag">{{ tag }}</span>
                </div>
              </div>

              <div class="task-actions">
                <button @click.stop="updateTask({ ...task, status: 'done' })" class="btn-sm">
                  Mark Done
                </button>
                <button @click.stop="deleteTask(task.id)" class="btn-sm btn-danger">
                  Delete
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Board View -->
      <div v-else-if="currentView === 'board'" class="board-view">
        <div 
          v-for="status in ['todo', 'in-progress', 'review', 'done']" 
          :key="status"
          class="board-column"
        >
          <h3 class="column-header">
            {{ status }}
            <span class="count">{{ sortedTasks.filter(t => t.status === status).length }}</span>
          </h3>
          <div class="board-cards">
            <div 
              v-for="task in sortedTasks.filter(t => t.status === status)"
              :key="task.id"
              class="board-card"
              :class="{ overdue: isOverdue(task) }"
            >
              <h4>{{ task.title }}</h4>
              <p>{{ task.description }}</p>
              <div class="card-footer">
                <span 
                  class="priority-indicator"
                  :style="{ backgroundColor: getPriorityColor(task.priority) }"
                ></span>
                <span class="assignee-avatar">{{ getUserById(task.assignee)?.name.charAt(0) }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Calendar View -->
      <div v-else-if="currentView === 'calendar'" class="calendar-view">
        <p>Calendar view coming soon...</p>
      </div>
    </main>

    <!-- Task Detail Sidebar -->
    <aside v-if="selectedTask" class="detail-sidebar">
      <div class="detail-header">
        <h2>Task Details</h2>
        <button @click="selectedTask = null" class="btn-close">×</button>
      </div>
      
      <div class="detail-content">
        <h3>{{ selectedTask.title }}</h3>
        <p>{{ selectedTask.description }}</p>
        
        <div class="detail-field">
          <label>Status</label>
          <select v-model="selectedTask.status" @change="updateTask(selectedTask)">
            <option value="todo">To Do</option>
            <option value="in-progress">In Progress</option>
            <option value="review">Review</option>
            <option value="done">Done</option>
          </select>
        </div>

        <div class="detail-field">
          <label>Priority</label>
          <select v-model="selectedTask.priority" @change="updateTask(selectedTask)">
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
            <option value="critical">Critical</option>
          </select>
        </div>

        <div class="detail-field">
          <label>Assignee</label>
          <select v-model="selectedTask.assignee" @change="updateTask(selectedTask)">
            <option v-for="user in users" :key="user.id" :value="user.id">
              {{ user.name }}
            </option>
          </select>
        </div>

        <div class="detail-field">
          <label>Due Date</label>
          <span>{{ formatDate(selectedTask.dueDate) }}</span>
        </div>

        <div class="detail-field">
          <label>Tags</label>
          <div class="tags">
            <span v-for="tag in selectedTask.tags" :key="tag" class="tag">{{ tag }}</span>
          </div>
        </div>

        <div class="detail-section">
          <h4>Comments ({{ selectedTask.comments.length }})</h4>
          <div v-if="selectedTask.comments.length === 0" class="empty">
            No comments yet
          </div>
          <div v-else class="comments-list">
            <div v-for="comment in selectedTask.comments" :key="comment.id" class="comment">
              <div class="comment-header">
                <strong>{{ getUserById(comment.userId)?.name }}</strong>
                <span class="timestamp">{{ formatDate(comment.timestamp) }}</span>
              </div>
              <p>{{ comment.text }}</p>
            </div>
          </div>
        </div>

        <div class="detail-section">
          <h4>Attachments ({{ selectedTask.attachments.length }})</h4>
          <div v-if="selectedTask.attachments.length === 0" class="empty">
            No attachments
          </div>
          <div v-else class="attachments-list">
            <div v-for="attachment in selectedTask.attachments" :key="attachment.id" class="attachment">
              <span>📎 {{ attachment.name }}</span>
              <span class="file-size">{{ (attachment.size / 1024).toFixed(1) }} KB</span>
            </div>
          </div>
        </div>
      </div>
    </aside>

    <!-- Task Modal -->
    <div v-if="showTaskModal" class="modal-overlay" @click="showTaskModal = false">
      <div class="modal" @click.stop>
        <div class="modal-header">
          <h2>Create New Task</h2>
          <button @click="showTaskModal = false" class="btn-close">×</button>
        </div>
        
        <div class="modal-body">
          <div class="form-field">
            <label>Title *</label>
            <input v-model="newTask.title" type="text" placeholder="Task title">
          </div>

          <div class="form-field">
            <label>Description</label>
            <textarea v-model="newTask.description" placeholder="Task description"></textarea>
          </div>

          <div class="form-row">
            <div class="form-field">
              <label>Priority</label>
              <select v-model="newTask.priority">
                <option value="low">Low</option>
                <option value="medium">Medium</option>
                <option value="high">High</option>
                <option value="critical">Critical</option>
              </select>
            </div>

            <div class="form-field">
              <label>Status</label>
              <select v-model="newTask.status">
                <option value="todo">To Do</option>
                <option value="in-progress">In Progress</option>
                <option value="review">Review</option>
                <option value="done">Done</option>
              </select>
            </div>
          </div>

          <div class="form-field">
            <label>Assignee</label>
            <select v-model="newTask.assignee">
              <option v-for="user in users" :key="user.id" :value="user.id">
                {{ user.name }}
              </option>
            </select>
          </div>
        </div>

        <div class="modal-footer">
          <button @click="showTaskModal = false" class="btn-secondary">Cancel</button>
          <button @click="createTask" :disabled="!newTask.title" class="btn-primary">Create</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
* {
  box-sizing: border-box;
}

.task-manager {
  display: grid;
  grid-template-areas:
    "header header header"
    "filters main detail";
  grid-template-columns: auto 1fr auto;
  grid-template-rows: auto 1fr;
  height: 100vh;
  background: #f5f7fa;
}

.header {
  grid-area: header;
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1.5rem 2rem;
  background: white;
  border-bottom: 1px solid #e1e8ed;
}

.filter-panel {
  grid-area: filters;
  width: 280px;
  background: white;
  padding: 1.5rem;
  border-right: 1px solid #e1e8ed;
  overflow-y: auto;
}

.main-content {
  grid-area: main;
  padding: 1.5rem;
  overflow-y: auto;
}

.detail-sidebar {
  grid-area: detail;
  width: 400px;
  background: white;
  border-left: 1px solid #e1e8ed;
  overflow-y: auto;
}

.stats-pills {
  display: flex;
  gap: 0.5rem;
  margin-top: 0.5rem;
}

.pill {
  padding: 0.25rem 0.75rem;
  border-radius: 12px;
  font-size: 0.875rem;
  background: #e1e8ed;
}

.pill.todo { background: #e3f2fd; }
.pill.in-progress { background: #fff3e0; }
.pill.done { background: #e8f5e9; }
.pill.overdue { background: #ffebee; color: #c62828; }

.btn-primary {
  padding: 0.5rem 1rem;
  background: #1976d2;
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}

.btn-secondary {
  padding: 0.5rem 1rem;
  background: white;
  border: 1px solid #e1e8ed;
  border-radius: 4px;
  cursor: pointer;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  padding: 1rem;
  background: white;
  border-radius: 4px;
  margin-bottom: 1rem;
}

.task-list {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.task-item {
  background: white;
  padding: 1rem;
  border-radius: 4px;
  border: 1px solid #e1e8ed;
  cursor: pointer;
}

.task-item:hover {
  box-shadow: 0 2px 8px rgba(0,0,0,0.1);
}

.task-item.selected {
  border-color: #1976d2;
  box-shadow: 0 0 0 2px rgba(25, 118, 210, 0.2);
}

.task-item.overdue {
  border-left: 4px solid #f44336;
}

.badge {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  font-size: 0.75rem;
  color: white;
}

.board-view {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 1rem;
  height: 100%;
}

.board-column {
  background: white;
  border-radius: 4px;
  padding: 1rem;
}

.board-cards {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  margin-top: 1rem;
}

.board-card {
  padding: 1rem;
  background: #f5f7fa;
  border-radius: 4px;
  border: 1px solid #e1e8ed;
}

.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal {
  background: white;
  border-radius: 8px;
  width: 600px;
  max-height: 80vh;
  overflow-y: auto;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1.5rem;
  border-bottom: 1px solid #e1e8ed;
}

.modal-body {
  padding: 1.5rem;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
  padding: 1.5rem;
  border-top: 1px solid #e1e8ed;
}

.form-field {
  margin-bottom: 1rem;
}

.form-field label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
}

.form-field input,
.form-field select,
.form-field textarea {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid #e1e8ed;
  border-radius: 4px;
}

.form-field textarea {
  min-height: 100px;
  resize: vertical;
}

.loading,
.error-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 3rem;
  text-align: center;
}

.spinner {
  width: 40px;
  height: 40px;
  border: 4px solid #e1e8ed;
  border-top-color: #1976d2;
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}
</style>
