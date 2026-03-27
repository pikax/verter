<script setup lang="ts">
import { ref, computed } from "vue";

const isLoading = ref(false);
const isAuthenticated = ref(false);
const userRole = ref<"admin" | "user" | "guest">("guest");
const hasPermission = ref(false);
const isOnline = ref(true);
const showDetails = ref(false);
const theme = ref<"light" | "dark">("light");
const error = ref<string | null>(null);

const canEdit = computed(
  () => isAuthenticated.value && (userRole.value === "admin" || hasPermission.value),
);
const statusMessage = computed(() => {
  if (!isOnline.value) return "Offline";
  if (isLoading.value) return "Loading...";
  if (error.value) return `Error: ${error.value}`;
  return "Ready";
});
</script>

<template>
  <div>
    <div v-if="isLoading">
      <p>Loading content...</p>
      <div class="spinner"></div>
    </div>
    <div v-else-if="error">
      <h2>Error</h2>
      <p>{{ error }}</p>
      <button @click="error = null">Dismiss</button>
    </div>
    <div v-else-if="!isOnline">
      <h2>Offline</h2>
      <p>Please check your connection</p>
    </div>
    <div v-else>
      <div v-if="isAuthenticated">
        <div v-if="userRole === 'admin'">
          <h2>Admin Panel</h2>
          <p>Full access granted</p>
        </div>
        <div v-else-if="userRole === 'user'">
          <h2>User Dashboard</h2>
          <div v-if="hasPermission">
            <p>Special permissions enabled</p>
          </div>
          <div v-else>
            <p>Standard access</p>
          </div>
        </div>
        <div v-else>
          <p>Authenticated as guest</p>
        </div>
      </div>
      <div v-else>
        <h2>Welcome</h2>
        <p>Please log in to continue</p>
      </div>

      <div v-if="showDetails">
        <div v-if="theme === 'dark'">
          <p>Dark mode enabled</p>
        </div>
        <div v-else-if="theme === 'light'">
          <p>Light mode enabled</p>
        </div>
      </div>

      <footer>
        <p>Status: {{ statusMessage }}</p>
        <button v-if="canEdit">Edit</button>
      </footer>
    </div>
  </div>
</template>
