<script setup lang="ts">
import { ref } from 'vue'
import TypedChild from './TypedChild.vue'

const parentMessage = ref('from-parent')
const received = ref('')

function onReply(value: string) {
  received.value = value
}

function updateMessage() {
  parentMessage.value = 'updated-parent'
}
</script>

<template>
  <div data-testid="typed-props-emits">
    <button data-testid="update-message" @click="updateMessage">Update Message</button>
    <span data-testid="received">{{ received }}</span>
    <TypedChild :message="parentMessage" @reply="onReply" />
  </div>
</template>
