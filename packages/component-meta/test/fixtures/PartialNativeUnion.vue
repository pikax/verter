<script setup lang="ts">
interface PropsObject {
  partialImage?: string | (Partial<HTMLImageElement> & { [key: string]: any })
}
defineProps<PropsObject>()
</script>

<template><div /></template>
