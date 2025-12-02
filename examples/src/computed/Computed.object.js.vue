<script>
import { defineComponent } from "vue";

export default defineComponent({
  name: "Computed",
  data() {
    return {
      firstName: "John",
      lastName: "Doe",
      count: 0,
      items: ["apple", "banana", "cherry"],
      price: 100,
      quantity: 1,
      numberList: [1, 5, 3, 9, 2, 7],
      status: "pending",
      baseValue: 10,
    };
  },
  computed: {
    // Simple readonly computed
    fullName() {
      return `${this.firstName} ${this.lastName}`;
    },
    // Computed with complex logic
    doubleCount() {
      return this.count * 2;
    },
    tripleCount() {
      return this.count * 3;
    },
    // Computed from array
    itemCount() {
      return this.items.length;
    },
    sortedItems() {
      return [...this.items].sort();
    },
    reversedItems() {
      return [...this.items].reverse();
    },
    upperCaseItems() {
      return this.items.map((item) => item.toUpperCase());
    },
    // Computed with filtering
    longItems() {
      return this.items.filter((item) => item.length > 5);
    },
    // Computed with multiple dependencies
    total() {
      return this.price * this.quantity;
    },
    totalWithTax() {
      return this.total * 1.1;
    },
    // Writable computed with getter/setter
    fullNameWritable: {
      get() {
        return `${this.firstName} ${this.lastName}`;
      },
      set(newValue) {
        const parts = newValue.split(" ");
        this.firstName = parts[0] || "";
        this.lastName = parts.slice(1).join(" ") || "";
      },
    },
    // Writable computed for formatted value
    formattedPrice: {
      get() {
        return `$${this.price.toFixed(2)}`;
      },
      set(newValue) {
        const numericValue = parseFloat(newValue.replace(/[^0-9.-]/g, ""));
        if (!isNaN(numericValue)) {
          this.price = numericValue;
        }
      },
    },
    // Computed with object return
    stats() {
      const nums = this.numberList;
      return {
        total: nums.reduce((a, b) => a + b, 0),
        average: nums.length ? nums.reduce((a, b) => a + b, 0) / nums.length : 0,
        max: Math.max(...nums),
        min: Math.min(...nums),
      };
    },
    // Computed with conditional logic
    statusDisplay() {
      switch (this.status) {
        case "pending":
          return "⏳ Pending";
        case "active":
          return "🔵 Active";
        case "completed":
          return "✅ Completed";
        default:
          return this.status;
      }
    },
    // Chained computed
    step1() {
      return this.baseValue + 5;
    },
    step2() {
      return this.step1 * 2;
    },
    step3() {
      return this.step2 - 3;
    },
    finalResult() {
      return `Result: ${this.step3}`;
    },
  },
  methods: {
    increment() {
      this.count++;
    },
    addItem(item) {
      this.items.push(item);
    },
    updateQuantity(newQuantity) {
      this.quantity = newQuantity;
    },
    cycleStatus() {
      const statuses = ["pending", "active", "completed"];
      const currentIndex = statuses.indexOf(this.status);
      this.status = statuses[(currentIndex + 1) % statuses.length];
    },
  },
});
</script>

<template>
  <div class="computed-demo">
    <section>
      <h3>Name Computed</h3>
      <p>Full Name (readonly): {{ fullName }}</p>
      <p>Full Name (writable): 
        <input v-model="fullNameWritable" />
      </p>
      <p>First: {{ firstName }}, Last: {{ lastName }}</p>
    </section>

    <section>
      <h3>Counter</h3>
      <p>Count: {{ count }}</p>
      <p>Double: {{ doubleCount }}, Triple: {{ tripleCount }}</p>
      <button @click="increment">Increment</button>
    </section>

    <section>
      <h3>Items ({{ itemCount }})</h3>
      <p>Original: {{ items }}</p>
      <p>Sorted: {{ sortedItems }}</p>
      <p>Reversed: {{ reversedItems }}</p>
      <p>Uppercase: {{ upperCaseItems }}</p>
      <p>Long (>5 chars): {{ longItems }}</p>
      <button @click="addItem('grape')">Add Grape</button>
    </section>

    <section>
      <h3>Price Calculator</h3>
      <p>
        Price: <input v-model="formattedPrice" />
      </p>
      <p>Quantity: <input type="number" v-model.number="quantity" /></p>
      <p>Subtotal: ${{ total.toFixed(2) }}</p>
      <p>Total (with tax): ${{ totalWithTax.toFixed(2) }}</p>
    </section>

    <section>
      <h3>Statistics</h3>
      <p>Numbers: {{ numberList }}</p>
      <p>Total: {{ stats.total }}, Average: {{ stats.average.toFixed(2) }}</p>
      <p>Max: {{ stats.max }}, Min: {{ stats.min }}</p>
    </section>

    <section>
      <h3>Status</h3>
      <p>{{ statusDisplay }}</p>
      <button @click="cycleStatus">Change Status</button>
    </section>

    <section>
      <h3>Chained Computed</h3>
      <p>Base: {{ baseValue }}</p>
      <p>+5: {{ step1 }} → ×2: {{ step2 }} → -3: {{ step3 }}</p>
      <p>{{ finalResult }}</p>
      <input type="number" v-model.number="baseValue" />
    </section>
  </div>
</template>
