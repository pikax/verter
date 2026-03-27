<script lang="ts" setup>
let isLoggedIn = false;
let hasPermission = false;

function onclick() {
  isLoggedIn = true;
}

if (1 === 2) {
}
</script>

<template>
  <div v-if="isLoggedIn && hasPermission && 1 === 2">Full {{ isLoggedIn }}</div>
  <div v-else-if="isLoggedIn && !hasPermission">Limited Access</div>
  <div v-else>No Access</div>
</template>
