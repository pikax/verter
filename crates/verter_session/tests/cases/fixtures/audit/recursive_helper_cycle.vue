<script setup lang="ts" generic="T extends object">
type DotPathKeys<T> = T extends object
  ? { [K in keyof T & string]: K | `${K}.${DotPathKeys<NonNullable<T[K]>>}` }[keyof T & string]
  : never
type GetItemKeys<T> = (keyof T & string) | DotPathKeys<T>
defineProps<{ key: GetItemKeys<T> }>()
</script>
<template><div /></template>
