<script setup>
const props = defineProps({
  msg: { type: String, default: "hi" },
  level: { type: Number, required: true },
})
const emit = defineEmits(["save", "cancel"])

function onSave() {
  emit("save", props.msg)
}
</script>

<template>
  <button @click="onSave">{{ msg }}:{{ level }}</button>
</template>
