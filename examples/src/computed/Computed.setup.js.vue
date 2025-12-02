<script setup>
import { ref, computed } from "vue";

// Basic reactive state
const firstName = ref("John");
const lastName = ref("Doe");
const count = ref(0);
const items = ref(["apple", "banana", "cherry"]);
const price = ref(100);
const quantity = ref(1);

// Simple readonly computed
const fullName = computed(() => {
  return `${firstName.value} ${lastName.value}`;
});

// Computed with complex logic
const doubleCount = computed(() => count.value * 2);
const tripleCount = computed(() => count.value * 3);

// Computed from array
const itemCount = computed(() => items.value.length);
const sortedItems = computed(() => [...items.value].sort());
const reversedItems = computed(() => [...items.value].reverse());
const upperCaseItems = computed(() => items.value.map((item) => item.toUpperCase()));

// Computed with filtering
const longItems = computed(() => items.value.filter((item) => item.length > 5));

// Computed with multiple dependencies
const total = computed(() => price.value * quantity.value);
const totalWithTax = computed(() => total.value * 1.1);

// Writable computed with getter/setter
const fullNameWritable = computed({
  get() {
    return `${firstName.value} ${lastName.value}`;
  },
  set(newValue) {
    const parts = newValue.split(" ");
    firstName.value = parts[0] || "";
    lastName.value = parts.slice(1).join(" ") || "";
  },
});

// Writable computed for formatted value
const formattedPrice = computed({
  get() {
    return `$${price.value.toFixed(2)}`;
  },
  set(newValue) {
    const numericValue = parseFloat(newValue.replace(/[^0-9.-]/g, ""));
    if (!isNaN(numericValue)) {
      price.value = numericValue;
    }
  },
});

// Computed with object return
const numberList = ref([1, 5, 3, 9, 2, 7]);

const stats = computed(() => {
  const nums = numberList.value;
  return {
    total: nums.reduce((a, b) => a + b, 0),
    average: nums.length ? nums.reduce((a, b) => a + b, 0) / nums.length : 0,
    max: Math.max(...nums),
    min: Math.min(...nums),
  };
});

// Computed with conditional logic
const status = ref("pending");
const statusDisplay = computed(() => {
  switch (status.value) {
    case "pending":
      return "⏳ Pending";
    case "active":
      return "🔵 Active";
    case "completed":
      return "✅ Completed";
    default:
      return status.value;
  }
});

// Chained computed
const baseValue = ref(10);
const step1 = computed(() => baseValue.value + 5);
const step2 = computed(() => step1.value * 2);
const step3 = computed(() => step2.value - 3);
const finalResult = computed(() => `Result: ${step3.value}`);

// Methods
function increment() {
  count.value++;
}

function addItem(item) {
  items.value.push(item);
}

function updateQuantity(newQuantity) {
  quantity.value = newQuantity;
}

function cycleStatus() {
  const statuses = ["pending", "active", "completed"];
  const currentIndex = statuses.indexOf(status.value);
  status.value = statuses[(currentIndex + 1) % statuses.length];
}
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
