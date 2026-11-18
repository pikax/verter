<script setup>
import { ref } from 'vue'
const count = ref(0)
function save() {}
const store = { reset() {} }
</script>
<template>
  <Panel @a="save" @b="save()" @c="count = 1" @d="(v) => count = v" @e="function (v) { count = v }" @f="save(); save()" @g="count = $event" @h="store.reset" @i="store.reset()" />
</template>
