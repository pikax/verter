<script setup lang="ts">
// Conditional rendering patterns for parser testing

import { ref } from "vue";

type Status = "pending" | "active" | "completed" | "error";
type UserRole = "admin" | "moderator" | "user" | "guest";

const status = ref<Status>("active");
const role = ref<UserRole>("admin");
const count = ref(5);
const isLoggedIn = ref(true);
const hasPermission = ref(true);
const items = ref<string[]>(["a", "b", "c"]);
const error = ref<Error | null>(null);
const data = ref<{ value: number } | undefined>({ value: 42 });
</script>

<template>
  <div>
    <h2>Conditional Rendering</h2>

    <!-- Simple v-if -->
    <section>
      <h3>Simple v-if</h3>
      <p v-if="isLoggedIn">Welcome back!</p>
    </section>

    <!-- v-if / v-else -->
    <section>
      <h3>v-if / v-else</h3>
      <p v-if="isLoggedIn">Logged in</p>
      <p v-else>Please log in</p>
    </section>

    <!-- v-if / v-else-if / v-else chain -->
    <section>
      <h3>v-if / v-else-if / v-else</h3>
      <p v-if="status === 'pending'">Pending...</p>
      <p v-else-if="status === 'active'">Active</p>
      <p v-else-if="status === 'completed'">Completed ✓</p>
      <p v-else-if="status === 'error'">Error!</p>
      <p v-else>Unknown status</p>
    </section>

    <!-- Multiple conditions with v-else-if -->
    <section>
      <h3>Role-based rendering</h3>
      <div v-if="role === 'admin'">Admin Dashboard</div>
      <div v-else-if="role === 'moderator'">Moderator Panel</div>
      <div v-else-if="role === 'user'">User Home</div>
      <div v-else>Guest View</div>
    </section>

    <!-- v-if with && expression -->
    <section>
      <h3>Combined conditions</h3>
      <p v-if="isLoggedIn && hasPermission">Full access granted</p>
      <p v-else-if="isLoggedIn && !hasPermission">Limited access</p>
      <p v-else>No access</p>
    </section>

    <!-- v-if with || expression -->
    <section>
      <h3>OR conditions</h3>
      <p v-if="role === 'admin' || role === 'moderator'">Has elevated privileges</p>
    </section>

    <!-- Numeric comparisons -->
    <section>
      <h3>Numeric conditions</h3>
      <p v-if="count === 0">No items</p>
      <p v-else-if="count === 1">One item</p>
      <p v-else-if="count < 5">Few items ({{ count }})</p>
      <p v-else-if="count < 10">Some items ({{ count }})</p>
      <p v-else>Many items ({{ count }})</p>
    </section>

    <!-- v-if with template -->
    <section>
      <h3>Template grouping</h3>
      <template v-if="isLoggedIn">
        <header>User Header</header>
        <main>User Content</main>
        <footer>User Footer</footer>
      </template>
      <template v-else>
        <header>Guest Header</header>
        <main>Guest Content</main>
      </template>
    </section>

    <!-- v-if with array length -->
    <section>
      <h3>Array length check</h3>
      <ul v-if="items.length > 0">
        <li v-for="item in items" :key="item">{{ item }}</li>
      </ul>
      <p v-else>No items to display</p>
    </section>

    <!-- Nullish checks -->
    <section>
      <h3>Nullish checks</h3>
      <p v-if="error">Error: {{ error.message }}</p>
      <p v-else-if="data">Data: {{ data.value }}</p>
      <p v-else>Loading...</p>
    </section>

    <!-- v-if with optional chaining -->
    <section>
      <h3>Optional chaining</h3>
      <p v-if="data?.value">Value exists: {{ data.value }}</p>
    </section>

    <!-- v-show (always renders, toggles display) -->
    <section>
      <h3>v-show</h3>
      <p v-show="isLoggedIn">This uses v-show (always in DOM)</p>
    </section>

    <!-- v-if vs v-show comparison -->
    <section>
      <h3>v-if vs v-show</h3>
      <!-- v-if: completely removed from DOM when false -->
      <div v-if="isLoggedIn" class="v-if-element">v-if element</div>
      <!-- v-show: always in DOM, display: none when false -->
      <div v-show="isLoggedIn" class="v-show-element">v-show element</div>
    </section>

    <!-- Nested conditionals -->
    <section>
      <h3>Nested conditionals</h3>
      <div v-if="isLoggedIn">
        <p v-if="role === 'admin'">Admin menu</p>
        <p v-else>User menu</p>
      </div>
    </section>
  </div>
</template>
