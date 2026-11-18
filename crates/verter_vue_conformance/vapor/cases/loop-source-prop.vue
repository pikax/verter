<script setup lang="ts">
defineProps({ entries: Array })
</script>
<template><div v-for="entry in entries || []">{{ entry }}</div></template>
