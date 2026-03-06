<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import MyComp from './MyComp.vue'
import BaseButton from './BaseButton.vue'
import WrappedButton from './WrappedButton.vue'
import FragmentComp from './FragmentComp.vue'
import NoInheritComp from './NoInheritComp.vue'
import ConditionalRoot from './ConditionalRoot.vue'
import FunctionalBtn from './FunctionalBtn'
import { formatCount } from './utils'

const count = ref(0)
const doubled = computed(() => count.value * 2)
const props = defineProps<{ title: string }>()
const formatted = formatCount(count.value)
const items = ref(['apple', 'banana', 'cherry'])
const inputVal = ref('')

onMounted(() => { console.log('mounted') })
watch(count, (val) => { console.log(val) })

function increment() { count.value++ }
function handleInput(e: Event) { console.log(e) }
function handleCustom(payload: string) { console.log(payload) }
</script>
<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <p>{{ formatted }}</p>
    <button @click="increment">+</button>
    <button @click.prevent="increment">prevent</button>
    <input @input="handleInput($event)" />
    <input v-model="inputVal" />
    <ul>
      <li v-for="(item, index) in items" :key="index">{{ item }}</li>
    </ul>
    <MyComp foo="literal" :bar="count" @custom="handleCustom($event)">
      <template #header>Header Content</template>
    </MyComp>
    <BaseButton label="click me" class="primary" />
    <WrappedButton variant="danger" class="extra" />
    <FragmentComp msg="hello" data-test="id" />
    <NoInheritComp label="ok" data-custom="val" />
    <ConditionalRoot :show="true" text="hi" class="cond" />
    <FunctionalBtn label="fn" class="fn-class" />
  </div>
</template>
