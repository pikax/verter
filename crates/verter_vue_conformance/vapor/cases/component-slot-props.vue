<script setup>
import { ref } from 'vue'
const name = ref('')
const rows = ref([])
</script>
<template>
  <Field v-slot="field">
    <TextInput :id="field.id" v-model="name" :describedby="field.describedby" />
  </Field>
  <Field v-slot="{ id, hint }">
    <input :id="id" v-model="name" :title="hint">
  </Field>
  <Table :rows="rows">
    <template #cell="{ row }"><b>{{ row.name }}</b></template>
  </Table>
</template>
