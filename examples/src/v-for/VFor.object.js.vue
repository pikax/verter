<script>
import { defineComponent } from "vue";

export default defineComponent({
  name: "VFor",
  data() {
    return {
      items: ["Apple", "Banana", "Cherry"],
      numbers: [1, 2, 3, 4, 5],
      user: {
        name: "John",
        age: 30,
        city: "New York",
      },
      users: [
        { id: 1, name: "Alice", email: "alice@example.com", active: true },
        { id: 2, name: "Bob", email: "bob@example.com", active: false },
        { id: 3, name: "Charlie", email: "charlie@example.com", active: true },
      ],
      categories: [
        {
          id: 1,
          name: "Electronics",
          products: [
            { id: 101, name: "Phone", price: 999 },
            { id: 102, name: "Laptop", price: 1499 },
          ],
        },
        {
          id: 2,
          name: "Clothing",
          products: [
            { id: 201, name: "Shirt", price: 49 },
            { id: 202, name: "Pants", price: 79 },
          ],
        },
      ],
      rangeCount: 5,
      itemRefs: [],
    };
  },
  computed: {
    activeUsers() {
      return this.users.filter((u) => u.active);
    },
    sortedNumbers() {
      return [...this.numbers].sort((a, b) => b - a);
    },
  },
  methods: {
    addItem() {
      this.items.push(`Item ${this.items.length + 1}`);
    },
    removeItem(index) {
      this.items.splice(index, 1);
    },
    toggleUser(id) {
      const user = this.users.find((u) => u.id === id);
      if (user) {
        user.active = !user.active;
      }
    },
    updateProduct(categoryId, productId, price) {
      const category = this.categories.find((c) => c.id === categoryId);
      const product = category?.products.find((p) => p.id === productId);
      if (product) {
        product.price = price;
      }
    },
    setItemRef(el, index) {
      if (el) {
        this.itemRefs[index] = el;
      }
    },
    focusItem(index) {
      this.itemRefs[index]?.focus();
    },
  },
});
</script>

<template>
  <div class="v-for-demo">
    <section>
      <h3>Array Iteration</h3>
      <ul>
        <li v-for="(item, index) in items" :key="index">
          {{ index }}: {{ item }}
          <button @click="removeItem(index)">×</button>
        </li>
      </ul>
      <button @click="addItem">Add Item</button>
    </section>

    <section>
      <h3>Users with ID Keys</h3>
      <div v-for="user in users" :key="user.id" class="user-card">
        <span>{{ user.name }} ({{ user.email }})</span>
        <span :class="{ active: user.active }">
          {{ user.active ? "Active" : "Inactive" }}
        </span>
        <button @click="toggleUser(user.id)">Toggle</button>
      </div>
    </section>

    <section>
      <h3>Object Iteration</h3>
      <dl>
        <template v-for="(value, key) in user" :key="key">
          <dt>{{ key }}</dt>
          <dd>{{ value }}</dd>
        </template>
      </dl>
      <ul>
        <li v-for="(value, key, index) in user" :key="key">
          {{ index }}. {{ key }}: {{ value }}
        </li>
      </ul>
    </section>

    <section>
      <h3>Range Iteration (n in {{ rangeCount }})</h3>
      <span v-for="n in rangeCount" :key="n" class="badge">
        {{ n }}
      </span>
      <input type="number" v-model.number="rangeCount" min="1" max="10" />
    </section>

    <section>
      <h3>Nested Iteration</h3>
      <div v-for="category in categories" :key="category.id" class="category">
        <h4>{{ category.name }}</h4>
        <ul>
          <li v-for="product in category.products" :key="product.id">
            {{ product.name }}: ${{ product.price }}
            <input
              type="number"
              :value="product.price"
              @input="updateProduct(category.id, product.id, Number($event.target.value))"
            />
          </li>
        </ul>
      </div>
    </section>

    <section>
      <h3>Computed Filtered List</h3>
      <p>Active users only:</p>
      <ul>
        <li v-for="user in activeUsers" :key="user.id">
          {{ user.name }}
        </li>
      </ul>
      <p>Sorted numbers (desc):</p>
      <span v-for="num in sortedNumbers" :key="num" class="badge">
        {{ num }}
      </span>
    </section>

    <section>
      <h3>v-for with v-if (using template wrapper)</h3>
      <ul>
        <template v-for="user in users" :key="user.id">
          <li v-if="user.active">
            {{ user.name }} (active)
          </li>
        </template>
      </ul>
    </section>

    <section>
      <h3>v-for with Template Refs</h3>
      <ul>
        <li
          v-for="(item, index) in items"
          :key="index"
          :ref="(el) => setItemRef(el, index)"
          tabindex="0"
        >
          {{ item }}
        </li>
      </ul>
      <button @click="focusItem(0)">Focus First</button>
    </section>

    <section>
      <h3>Destructuring in v-for</h3>
      <ul>
        <li v-for="{ id, name, email } in users" :key="id">
          {{ name }} &lt;{{ email }}&gt;
        </li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
.v-for-demo section {
  margin-bottom: 20px;
}

.user-card {
  display: flex;
  gap: 10px;
  padding: 10px;
  border: 1px solid #ccc;
  margin: 5px 0;
}

.active {
  color: green;
}

.badge {
  display: inline-block;
  padding: 4px 8px;
  margin: 2px;
  background: #eee;
  border-radius: 4px;
}

.category {
  margin: 10px 0;
  padding: 10px;
  border: 1px solid #ddd;
}
</style>
