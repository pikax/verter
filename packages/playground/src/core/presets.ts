import type { SerializedState } from "./urlState";

export interface Preset {
  name: string;
  description: string;
  /** The framework adapter id this preset targets (descriptor-driven, e.g. "vue" / "svelte"). */
  language: string;
  /** The carrier (main) filename for this preset, e.g. "App.vue" / "App.svelte". */
  mainFile: string;
  state: SerializedState;
}

export const presets: Preset[] = [
  {
    name: "Counter",
    description: "Basic ref() + template binding",
    language: "vue",
    mainFile: "App.vue",
    state: {
      files: {
        "App.vue": `<script setup lang="ts">
import { ref } from 'vue'

const count = ref(0)
const message = ref('Hello from Verter!')

function increment() {
  count.value++
}
</script>

<template>
  <div class="app">
    <h1>{{ message }}</h1>
    <button @click="increment">Count: {{ count }}</button>
  </div>
</template>

<style scoped>
.app {
  font-family: sans-serif;
  text-align: center;
  padding: 2rem;
}
button {
  padding: 0.5rem 1rem;
  font-size: 1rem;
  cursor: pointer;
}
</style>
`,
      },
      activeFile: "App.vue",
      outputMode: "preview",
      compilerOptions: { isProduction: false, ssr: false },
      language: "vue",
    },
  },
  {
    name: "TypeScript Props",
    description: "defineProps<T> + withDefaults across 2 files",
    language: "vue",
    mainFile: "App.vue",
    state: {
      files: {
        "App.vue": `<script setup lang="ts">
import Greeting from './Greeting.vue'
</script>

<template>
  <Greeting name="Verter" :count="42" />
  <Greeting name="World" />
</template>
`,
        "Greeting.vue": `<script setup lang="ts">
interface Props {
  name: string
  count?: number
  variant?: 'primary' | 'secondary'
}

const props = withDefaults(defineProps<Props>(), {
  count: 0,
  variant: 'primary',
})
</script>

<template>
  <div :class="variant">
    <h2>Hello, {{ name }}!</h2>
    <p v-if="count > 0">Count: {{ count }}</p>
  </div>
</template>

<style scoped>
.primary { color: #42b883; }
.secondary { color: #aaa; }
</style>
`,
      },
      activeFile: "App.vue",
      outputMode: "preview",
      compilerOptions: { isProduction: false, ssr: false },
      language: "vue",
    },
  },
  {
    name: "Composable",
    description: "Custom useMouse() composable + consuming component",
    language: "vue",
    mainFile: "App.vue",
    state: {
      files: {
        "App.vue": `<script setup lang="ts">
import { useMouse } from './useMouse'

const { x, y } = useMouse()
</script>

<template>
  <div class="app">
    <h1>Mouse Tracker</h1>
    <p>Position: ({{ x }}, {{ y }})</p>
  </div>
</template>

<style scoped>
.app {
  font-family: sans-serif;
  text-align: center;
  padding: 2rem;
}
</style>
`,
        "useMouse.ts": `import { ref, onMounted, onUnmounted } from 'vue'

export function useMouse() {
  const x = ref(0)
  const y = ref(0)

  function update(event: MouseEvent) {
    x.value = event.pageX
    y.value = event.pageY
  }

  onMounted(() => window.addEventListener('mousemove', update))
  onUnmounted(() => window.removeEventListener('mousemove', update))

  return { x, y }
}
`,
      },
      activeFile: "App.vue",
      outputMode: "preview",
      compilerOptions: { isProduction: false, ssr: false },
      language: "vue",
    },
  },
  {
    name: "Multi-Component",
    description: "Parent + child with events and slots",
    language: "vue",
    mainFile: "App.vue",
    state: {
      files: {
        "App.vue": `<script setup lang="ts">
import { ref } from 'vue'
import Card from './Card.vue'

const items = ref(['Vue', 'Verter', 'TypeScript'])

function handleRemove(index: number) {
  items.value.splice(index, 1)
}
</script>

<template>
  <div class="app">
    <h1>Items</h1>
    <Card
      v-for="(item, i) in items"
      :key="item"
      :title="item"
      @remove="handleRemove(i)"
    >
      <p>Item #{{ i + 1 }}</p>
    </Card>
  </div>
</template>

<style scoped>
.app {
  font-family: sans-serif;
  padding: 2rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  align-items: center;
}
</style>
`,
        "Card.vue": `<script setup lang="ts">
defineProps<{
  title: string
}>()

const emit = defineEmits<{
  remove: []
}>()
</script>

<template>
  <div class="card">
    <div class="header">
      <h3>{{ title }}</h3>
      <button @click="emit('remove')">x</button>
    </div>
    <slot />
  </div>
</template>

<style scoped>
.card {
  border: 1px solid #ddd;
  border-radius: 8px;
  padding: 1rem;
  width: 200px;
}
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
</style>
`,
      },
      activeFile: "App.vue",
      outputMode: "preview",
      compilerOptions: { isProduction: false, ssr: false },
      language: "vue",
    },
  },
  {
    name: "CSS Modules",
    description: "<style module> with $style binding",
    language: "vue",
    mainFile: "App.vue",
    state: {
      files: {
        "App.vue": `<script setup lang="ts">
import { ref } from 'vue'

const active = ref(false)
</script>

<template>
  <div :class="$style.container">
    <h1 :class="$style.title">CSS Modules Demo</h1>
    <button
      :class="[$style.btn, active ? $style.active : '']"
      @click="active = !active"
    >
      {{ active ? 'Active' : 'Inactive' }}
    </button>
  </div>
</template>

<style module>
.container {
  font-family: sans-serif;
  text-align: center;
  padding: 2rem;
}
.title {
  color: #42b883;
}
.btn {
  padding: 0.5rem 1.5rem;
  font-size: 1rem;
  border: 2px solid #42b883;
  border-radius: 6px;
  background: white;
  color: #42b883;
  cursor: pointer;
  transition: all 0.2s;
}
.active {
  background: #42b883;
  color: white;
}
</style>
`,
      },
      activeFile: "App.vue",
      outputMode: "preview",
      compilerOptions: { isProduction: false, ssr: false },
      language: "vue",
    },
  },
  {
    name: "Slots & Emits",
    description: "defineSlots + defineEmits patterns",
    language: "vue",
    mainFile: "App.vue",
    state: {
      files: {
        "App.vue": `<script setup lang="ts">
import { ref } from 'vue'
import DataList from './DataList.vue'

const items = ref([
  { id: 1, label: 'Alpha' },
  { id: 2, label: 'Beta' },
  { id: 3, label: 'Gamma' },
])

function handleSelect(id: number) {
  alert('Selected: ' + id)
}
</script>

<template>
  <DataList :items="items" @select="handleSelect">
    <template #header>
      <h1>My List</h1>
    </template>
    <template #item="{ item }">
      <strong>{{ item.label }}</strong> (id: {{ item.id }})
    </template>
  </DataList>
</template>
`,
        "DataList.vue": `<script setup lang="ts" generic="T extends { id: number }">
defineProps<{
  items: T[]
}>()

defineEmits<{
  select: [id: number]
}>()

defineSlots<{
  header(): any
  item(props: { item: T }): any
}>()
</script>

<template>
  <div class="list">
    <slot name="header" />
    <ul>
      <li
        v-for="item in items"
        :key="item.id"
        @click="$emit('select', item.id)"
      >
        <slot name="item" :item="item" />
      </li>
    </ul>
  </div>
</template>

<style scoped>
.list { font-family: sans-serif; padding: 1rem; }
ul { list-style: none; padding: 0; }
li {
  padding: 0.5rem;
  border-bottom: 1px solid #eee;
  cursor: pointer;
}
li:hover { background: #f5f5f5; }
</style>
`,
      },
      activeFile: "App.vue",
      outputMode: "preview",
      compilerOptions: { isProduction: false, ssr: false },
      language: "vue",
    },
  },

  // ── Svelte (experimental) ──────────────────────────────────────────────
  // Svelte presets default to a non-preview output mode: runtime preview is not
  // yet supported for Svelte, so they showcase the compiled/analysis surfaces.
  {
    name: "Svelte Counter",
    description: "$state rune + markup binding",
    language: "svelte",
    mainFile: "App.svelte",
    state: {
      files: {
        "App.svelte": `<script lang="ts">
  let count = $state(0)
</script>

<button onclick={() => count++}>
  Clicked {count} {count === 1 ? 'time' : 'times'}
</button>
`,
      },
      activeFile: "App.svelte",
      outputMode: "files",
      compilerOptions: { isProduction: false, ssr: false },
      language: "svelte",
    },
  },
  {
    name: "Svelte Runes",
    description: "$state / $derived / $effect",
    language: "svelte",
    mainFile: "App.svelte",
    state: {
      files: {
        "App.svelte": `<script lang="ts">
  let count = $state(0)
  let doubled = $derived(count * 2)

  $effect(() => {
    console.log('count is now', count)
  })
</script>

<button onclick={() => count++}>increment</button>
<p>{count} doubled is {doubled}</p>
`,
      },
      activeFile: "App.svelte",
      outputMode: "files",
      compilerOptions: { isProduction: false, ssr: false },
      language: "svelte",
    },
  },
  {
    name: "Svelte Props",
    description: "$props() rune across 2 components",
    language: "svelte",
    mainFile: "App.svelte",
    state: {
      files: {
        "App.svelte": `<script lang="ts">
  import Greeting from './Greeting.svelte'
</script>

<Greeting name="Verter" count={42} />
<Greeting name="World" />
`,
        "Greeting.svelte": `<script lang="ts">
  interface Props {
    name: string
    count?: number
  }

  let { name, count = 0 }: Props = $props()
</script>

<h2>Hello, {name}!</h2>
{#if count > 0}
  <p>Count: {count}</p>
{/if}
`,
      },
      activeFile: "App.svelte",
      outputMode: "files",
      compilerOptions: { isProduction: false, ssr: false },
      language: "svelte",
    },
  },
  {
    name: "Svelte Snippets",
    description: "{#snippet} + {@render}",
    language: "svelte",
    mainFile: "App.svelte",
    state: {
      files: {
        "App.svelte": `<script lang="ts">
  let items = $state(['Svelte', 'Verter', 'TypeScript'])
</script>

{#snippet row(label: string, index: number)}
  <li>#{index + 1}: {label}</li>
{/snippet}

<ul>
  {#each items as item, i}
    {@render row(item, i)}
  {/each}
</ul>
`,
      },
      activeFile: "App.svelte",
      outputMode: "files",
      compilerOptions: { isProduction: false, ssr: false },
      language: "svelte",
    },
  },
  {
    name: "Svelte Rune Module",
    description: ".svelte.ts rune module + consuming component",
    language: "svelte",
    mainFile: "App.svelte",
    state: {
      files: {
        "App.svelte": `<script lang="ts">
  import { counter } from './counter.svelte'
</script>

<button onclick={() => counter.increment()}>
  count is {counter.value}
</button>
`,
        "counter.svelte.ts": `export function createCounter() {
  let value = $state(0)
  return {
    get value() {
      return value
    },
    increment() {
      value += 1
    },
  }
}

export const counter = createCounter()
`,
      },
      activeFile: "App.svelte",
      outputMode: "files",
      compilerOptions: { isProduction: false, ssr: false },
      language: "svelte",
    },
  },
];
