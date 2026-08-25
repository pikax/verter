<script lang="ts">
class Thing {
  id = 1
}
export default {
  props: {
    label: String,
    count: { type: Number, required: true },
    flag: { type: Boolean },
    value: [String, Number],
    maybe: [String, null],
    size: { type: String, default: 'md' },
    when: Date,
    mixed: [String, Date],
    thing: Thing,
  },
}
</script>
<template><div /></template>
