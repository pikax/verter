<script setup>
import { ref } from 'vue'
const body = ref('')
const id = ref('')
const ok = ref(false)
const items = ref([])
const name = ref('n')
</script>
<template>
  <Card><template #header>H</template>{{ body }}</Card>
  <Card><template #header>H</template><p :id="id">t</p></Card>
  <Child><Foo v-if="ok" /></Child>
  <Child><template #a><Foo v-if="ok" /></template><template #b><slot /></template></Child>
  <Comp><template v-for="i in items" #[i]><span ref="row">{{ i }}</span></template></Comp>
  <Comp><template v-if="ok" #a>A</template><template #[name]>B</template><template #c>C</template></Comp>
</template>
