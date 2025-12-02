<script setup lang="ts">
import SlotsPatterns from "./SlotsPatterns.vue";
import GenericSlots from "./GenericSlots.vue";

interface User {
  id: number;
  name: string;
  email: string;
}

const users: User[] = [
  { id: 1, name: "John", email: "john@example.com" },
  { id: 2, name: "Jane", email: "jane@example.com" },
];
</script>

<template>
  <div>
    <h2>Slot Usage Examples</h2>

    <!-- Basic slot usage -->
    <SlotsPatterns>
      <template #default>
        <p>Default slot content</p>
      </template>

      <template #header>
        <h3>Header Slot</h3>
      </template>

      <template #footer>
        <p>Footer content</p>
      </template>
    </SlotsPatterns>

    <hr />

    <!-- Generic slots with type inference -->
    <GenericSlots :items="users">
      <template #header="{ count }">
        <h3>{{ count }} users</h3>
      </template>

      <template #default="{ item, index }">
        <!-- item is typed as User -->
        <div>
          #{{ index }}: {{ item.name }} ({{ item.email }})
        </div>
      </template>

      <template #empty>
        <p>No users found</p>
      </template>

      <template #footer="{ items }">
        <p>Total: {{ items.length }} users</p>
      </template>
    </GenericSlots>

    <!-- Dynamic slot names -->
    <GenericSlots :items="[]">
      <template v-for="name in ['header', 'empty', 'footer']" #[name]>
        <div>Dynamic slot: {{ name }}</div>
      </template>
    </GenericSlots>

    <!-- Shorthand syntax -->
    <GenericSlots :items="users" v-slot="{ item }">
      {{ item.name }}
    </GenericSlots>
  </div>
</template>
