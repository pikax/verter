<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import MyComp from "@/components/MyComp.vue";

const count = ref(0);
const doubled = computed(() => count.value * 2);
const props = defineProps<{ title: string }>();

onMounted(() => {
  console.log("mounted");
});
watch(count, (val) => {
  console.log(val);
});

function increment() {
  count.value++;
}
</script>
<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <button @click="increment">+</button>
    <MyComp foo="literal" :bar="count" />
  </div>
</template>
