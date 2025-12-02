<script>
// Conditional rendering patterns for parser testing - Options API + JavaScript

import { defineComponent } from "vue";

export default defineComponent({
  name: "ConditionalsObjectJs",

  data() {
    return {
      // Boolean conditions
      isVisible: true,
      isEnabled: false,
      isLoggedIn: true,

      // State for v-else-if chains
      status: "pending", // 'pending' | 'success' | 'error' | 'loading'

      // Numeric conditions
      count: 5,
      threshold: 10,

      // Object/Array conditions
      user: { name: "John", role: "admin" },
      items: ["a", "b", "c"],

      // Nested conditions
      features: {
        darkMode: true,
        notifications: false,
        beta: {
          enabled: true,
          features: ["feature1", "feature2"],
        },
      },

      // Optional chaining in conditions
      config: {
        settings: { advanced: true },
      },
    };
  },

  computed: {
    hasPermission() {
      return this.user?.role === "admin";
    },

    isWithinLimit() {
      return this.count <= this.threshold;
    },
  },

  methods: {
    toggle() {
      this.isVisible = !this.isVisible;
    },

    setStatus(newStatus) {
      this.status = newStatus;
    },
  },
});
</script>

<template>
  <div>
    <!-- Basic v-if -->
    <div v-if="isVisible">Visible content</div>

    <!-- v-if with v-else -->
    <div v-if="isLoggedIn">Welcome back!</div>
    <div v-else>Please log in</div>

    <!-- v-if, v-else-if, v-else chain -->
    <div v-if="status === 'loading'">Loading...</div>
    <div v-else-if="status === 'success'">Success!</div>
    <div v-else-if="status === 'error'">Error occurred</div>
    <div v-else>Pending...</div>

    <!-- v-show (always rendered, CSS display toggle) -->
    <div v-show="isEnabled">This is hidden with CSS when disabled</div>

    <!-- Multiple conditions combined -->
    <div v-if="isLoggedIn && hasPermission">Admin panel access</div>
    <div v-else-if="isLoggedIn && !hasPermission">Regular user access</div>
    <div v-else>Guest access</div>

    <!-- Numeric comparisons -->
    <span v-if="count > threshold">Over threshold</span>
    <span v-else-if="count === threshold">At threshold</span>
    <span v-else>Under threshold</span>

    <!-- Null/undefined checks -->
    <div v-if="user">User: {{ user.name }}</div>
    <div v-else>No user</div>

    <!-- Optional chaining in template -->
    <div v-if="config?.settings?.advanced">Advanced mode enabled</div>

    <!-- Array length checks -->
    <ul v-if="items.length > 0">
      <li v-for="item in items" :key="item">{{ item }}</li>
    </ul>
    <p v-else>No items</p>

    <!-- Nested conditionals -->
    <div v-if="features.darkMode">
      <span>Dark mode ON</span>
      <div v-if="features.beta.enabled">
        Beta features:
        <span v-for="f in features.beta.features" :key="f">{{ f }}</span>
      </div>
    </div>

    <!-- v-if on template (grouping without wrapper element) -->
    <template v-if="isVisible">
      <h1>Title</h1>
      <p>Paragraph 1</p>
      <p>Paragraph 2</p>
    </template>

    <!-- v-if vs v-show comparison -->
    <div v-if="isEnabled">v-if: Removed from DOM when false</div>
    <div v-show="isEnabled">v-show: Hidden with CSS when false</div>

    <!-- v-if with computed -->
    <button v-if="isWithinLimit" @click="toggle">Within limit</button>
    <button v-else disabled>Limit exceeded</button>

    <!-- Status buttons for testing -->
    <button @click="setStatus('loading')">Set Loading</button>
    <button @click="setStatus('success')">Set Success</button>
    <button @click="setStatus('error')">Set Error</button>
    <button @click="setStatus('pending')">Set Pending</button>
  </div>
</template>
