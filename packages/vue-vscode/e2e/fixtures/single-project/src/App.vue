<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import MyComp from './MyComp.vue'
import BaseButton from './BaseButton.vue'
import WrappedButton from './WrappedButton.vue'
import FragmentComp from './FragmentComp.vue'
import NoInheritComp from './NoInheritComp.vue'
import ConditionalRoot from './ConditionalRoot.vue'
import FunctionalBtn from './FunctionalBtn'
import GenericAttrsComp from './GenericAttrsComp.vue'
import { formatCount } from './utils'

interface Action { label: string; disabled: boolean; handler: () => void }
interface User { name: string; email: string; age: number }

const count = ref(0)
const doubled = computed(() => count.value * 2)
const props = defineProps<{ title: string }>()
const formatted = formatCount(count.value)
const items = ref(['apple', 'banana', 'cherry'])
const inputVal = ref('')
const actions = ref<Action[]>([{ label: 'ok', disabled: false, handler: () => {} }])
const users = ref<User[]>([{ name: 'Alice', email: 'a@b.com', age: 30 }])
const selectedUser = ref<User | null>(null)

onMounted(() => { console.log('mounted') })
watch(count, (val) => { console.log(val) })

function increment() { count.value++ }
function handleInput(e: Event) { console.log(e) }
function handleCustom(payload: string) { console.log(payload) }
</script>
<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ props.title }}</p>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <!-- Broken expression for Unknown context testing -->
    <p>{{ count + }}</p>
    <p>{{ formatted }}</p>
    <button @click="increment">+</button>
    <button @click.prevent="increment">prevent</button>
    <input @input="handleInput($event)" />
    <input v-model="inputVal" />
    <ul>
      <li v-for="(item, index) in items" :key="index">{{ item }}</li>
    </ul>
    <!-- v-for with typed member access -->
    <button v-for="action in actions" :key="action.label" :disabled="action.disabled">{{ action.label }}</button>
    <!-- v-for with destructured params -->
    <div v-for="{ name, email } in users" :key="email">{{ name }} ({{ email }})</div>
    <!-- v-for with index and member access -->
    <span v-for="(user, idx) in users" :key="idx">{{ user.name }}</span>
    <!-- Nested v-for -->
    <div v-for="user in users" :key="user.email">
      <button v-for="action in actions" :key="action.label" @click="action.handler">
        {{ user.name }}: {{ action.label }}
      </button>
    </div>
    <!-- v-if with member access -->
    <p v-if="selectedUser">{{ selectedUser.name }}</p>
    <MyComp foo="literal" :bar="count" @custom="handleCustom($event)">
      <template #header>Header Content</template>
    </MyComp>
    <BaseButton label="click me" class="primary" />
    <WrappedButton variant="danger" class="extra" />
    <FragmentComp msg="hello" data-test="id" />
    <NoInheritComp label="ok" data-custom="val" />
    <ConditionalRoot :show="true" text="hi" class="cond" />
    <FunctionalBtn label="fn" class="fn-class" />
    <GenericAttrsComp :value="'hello'" class="generic-test" />
  </div>
</template>
