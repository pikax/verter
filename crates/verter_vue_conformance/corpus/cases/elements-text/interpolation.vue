<script setup>
import { computed, ref } from "vue"

const count = ref(1)
const doubled = computed(() => count.value * 2)
</script>

<template>
  <p>
    Count: {{ count }} / Doubled: {{ doubled }} / Upper:
    {{ "hi".toUpperCase() }} / Sign: {{ count > 1 ? "many" : "one" }}
  </p>
</template>
