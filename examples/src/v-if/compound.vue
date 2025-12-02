<script lang="ts" setup>
let isLoggedIn = false;
let hasPermission = false;
</script>
<template>
  <div v-if="isLoggedIn && hasPermission">Full Access</div>
  <div v-else-if="isLoggedIn && !hasPermission">Limited Access</div>
  <div v-else>No Access</div>
</template>
