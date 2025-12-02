<script setup lang="ts">
// Complex v-for patterns for parser testing

interface Item {
  id: number;
  name: string;
  children?: Item[];
}

interface User {
  id: number;
  name: string;
  email: string;
  roles: string[];
}

const items: Item[] = [
  { id: 1, name: "First", children: [{ id: 11, name: "Child 1" }] },
  { id: 2, name: "Second" },
];

const users: User[] = [
  { id: 1, name: "John", email: "john@example.com", roles: ["admin"] },
  { id: 2, name: "Jane", email: "jane@example.com", roles: ["user", "editor"] },
];

const config: Record<string, string | number | boolean> = {
  theme: "dark",
  count: 42,
  enabled: true,
};

const matrix: number[][] = [
  [1, 2, 3],
  [4, 5, 6],
  [7, 8, 9],
];

const nestedMap = new Map<string, Map<string, number>>([
  ["group1", new Map([["a", 1], ["b", 2]])],
  ["group2", new Map([["c", 3], ["d", 4]])],
]);

// Set iteration
const uniqueIds = new Set([1, 2, 3, 4, 5]);

// Range helper
function range(start: number, end: number): number[] {
  return Array.from({ length: end - start + 1 }, (_, i) => start + i);
}
</script>

<template>
  <div>
    <h2>v-for Patterns</h2>

    <!-- Basic array iteration -->
    <section>
      <h3>Basic Array</h3>
      <ul>
        <li v-for="item in items" :key="item.id">{{ item.name }}</li>
      </ul>
    </section>

    <!-- With index -->
    <section>
      <h3>With Index</h3>
      <ul>
        <li v-for="(item, index) in items" :key="item.id">
          {{ index }}: {{ item.name }}
        </li>
      </ul>
    </section>

    <!-- Destructuring in v-for -->
    <section>
      <h3>Destructuring</h3>
      <ul>
        <li v-for="{ id, name, email } in users" :key="id">
          {{ name }} ({{ email }})
        </li>
      </ul>
    </section>

    <!-- Destructuring with index -->
    <section>
      <h3>Destructuring with Index</h3>
      <ul>
        <li v-for="({ id, name }, index) in users" :key="id">
          #{{ index }}: {{ name }}
        </li>
      </ul>
    </section>

    <!-- Nested destructuring -->
    <section>
      <h3>Nested Destructuring</h3>
      <ul>
        <li v-for="{ id, roles: [primaryRole] } in users" :key="id">
          Primary role: {{ primaryRole }}
        </li>
      </ul>
    </section>

    <!-- Object iteration -->
    <section>
      <h3>Object Iteration</h3>
      <ul>
        <li v-for="(value, key) in config" :key="key">
          {{ key }}: {{ value }}
        </li>
      </ul>
    </section>

    <!-- Object with index -->
    <section>
      <h3>Object with Index</h3>
      <ul>
        <li v-for="(value, key, index) in config" :key="key">
          {{ index }}. {{ key }} = {{ value }}
        </li>
      </ul>
    </section>

    <!-- Range iteration -->
    <section>
      <h3>Range (1-10)</h3>
      <span v-for="n in 10" :key="n">{{ n }} </span>
    </section>

    <!-- Function-generated range -->
    <section>
      <h3>Custom Range (5-10)</h3>
      <span v-for="n in range(5, 10)" :key="n">{{ n }} </span>
    </section>

    <!-- Nested v-for -->
    <section>
      <h3>Nested (Matrix)</h3>
      <div v-for="(row, rowIdx) in matrix" :key="rowIdx">
        <span v-for="(cell, colIdx) in row" :key="`${rowIdx}-${colIdx}`">
          [{{ cell }}]
        </span>
      </div>
    </section>

    <!-- Recursive children -->
    <section>
      <h3>Nested Children</h3>
      <ul>
        <template v-for="item in items" :key="item.id">
          <li>{{ item.name }}</li>
          <ul v-if="item.children">
            <li v-for="child in item.children" :key="child.id">
              ↳ {{ child.name }}
            </li>
          </ul>
        </template>
      </ul>
    </section>

    <!-- v-for with v-if (using template) -->
    <section>
      <h3>v-for with v-if (template)</h3>
      <ul>
        <template v-for="user in users" :key="user.id">
          <li v-if="user.roles.includes('admin')">
            {{ user.name }} [ADMIN]
          </li>
        </template>
      </ul>
    </section>

    <!-- Set iteration -->
    <section>
      <h3>Set Iteration</h3>
      <span v-for="id in uniqueIds" :key="id">{{ id }} </span>
    </section>

    <!-- v-for on component -->
    <section>
      <h3>Component Iteration</h3>
      <UserCard
        v-for="user in users"
        :key="user.id"
        :user="user"
        @click="() => console.log(user.id)"
      />
    </section>
  </div>
</template>

<script setup lang="ts">
import { defineComponent } from "vue";

// Placeholder component definition for iteration
const UserCard = defineComponent({
  props: {
    user: { type: Object as () => User, required: true },
  },
  template: `<div>{{ user.name }}</div>`,
});
</script>
