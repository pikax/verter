<script setup>
import { ref } from 'vue'
defineProps({ owner: Number })
const points = ref([3, 1, 2])
const seen = new Set([1])
const book = { current: [{ model: 'a' }] }
const limit = ref(2)
const groups = ref([[1, 2]])
</script>
<template>
  <div>
    <i v-for="p in [...points].reverse()" :key="p">{{ p }}</i>
    <i v-for="n in points.filter(n => seen.has(n) || n > limit)" :key="n">{{ n }}</i>
    <i v-for="m in [...new Set([...book.current.map(row => row.model)])]" :key="m">{{ m }}</i>
    <i v-for="o in points.filter(limit => limit !== owner)" :key="o">{{ o }}</i>
    <i v-for="f in (function () { return arguments[0] })(points)" :key="f">{{ f }}</i>
    <i v-for="e in \u0070oints" :key="e">{{ e }}</i>
    <p v-for="(row, k) in groups" :key="k">
      <b v-for="c in row.filter(x => x !== row.length && x < limit)" :key="c">{{ c }}</b>
    </p>
  </div>
</template>
