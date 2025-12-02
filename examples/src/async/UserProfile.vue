<script setup lang="ts">
// User profile component stub
defineProps<{
  userId: number;
  showAvatar?: boolean;
}>();
</script>

<template>
  <div class="user-profile">
    <p>User ID: {{ userId }}</p>
    <div v-if="showAvatar" class="avatar">Avatar</div>
  </div>
</template>
