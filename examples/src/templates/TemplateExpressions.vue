<script setup lang="ts">
// Template expression patterns for parser testing

const count = 1 as number;
const name = "John";
const items = ["a", "b", "c"];
const user = { name: "John", age: 30, address: { city: "NYC" } } as Record<string, any>;
const fn = (x: number) => x * 2;
const condition = true;
const nullableValue: string | null = "value";
const undefinedValue: string | undefined = undefined;

// Function declarations for template use
function formatDate(date: Date): string {
  return date.toISOString();
}

function greet(name: string, formal = false): string {
  return formal ? `Dear ${name}` : `Hi ${name}`;
}

async function fetchData(): Promise<string> {
  return "data";
}

// Getter-style computed
const now = new Date();
</script>

<template>
  <div>
    <h2>Template Expressions</h2>

    <!-- Simple interpolation -->
    <p>{{ count }}</p>
    <p>{{ name }}</p>

    <!-- Expressions -->
    <p>{{ count + 1 }}</p>
    <p>{{ count * 2 + 10 }}</p>
    <p>{{ name.toUpperCase() }}</p>
    <p>{{ name.length }}</p>

    <!-- Template literals -->
    <p>{{ `Hello, ${name}!` }}</p>
    <p>{{ `Count is ${count}, doubled: ${count * 2}` }}</p>

    <!-- Ternary -->
    <p>{{ condition ? "Yes" : "No" }}</p>
    <p>{{ count > 0 ? "Positive" : count < 0 ? "Negative" : "Zero" }}</p>

    <!-- Logical operators -->
    <p>{{ condition && "Shown" }}</p>
    <p>{{ nullableValue || "Default" }}</p>
    <p>{{ nullableValue ?? "Nullish default" }}</p>

    <!-- Optional chaining -->
    <p>{{ user?.name }}</p>
    <p>{{ user?.address?.city }}</p>
    <p>{{ user?.nonexistent?.value }}</p>

    <!-- Array operations -->
    <p>{{ items.join(", ") }}</p>
    <p>{{ items.map(i => i.toUpperCase()).join(" | ") }}</p>
    <p>{{ items.filter(i => i !== "b") }}</p>
    <p>{{ items.length }}</p>
    <p>{{ items[0] }}</p>
    <p>{{ items.at(-1) }}</p>

    <!-- Object operations -->
    <p>{{ Object.keys(user).join(", ") }}</p>
    <p>{{ JSON.stringify(user) }}</p>

    <!-- Function calls -->
    <p>{{ formatDate(now) }}</p>
    <p>{{ greet(name) }}</p>
    <p>{{ greet(name, true) }}</p>
    <p>{{ fn(5) }}</p>

    <!-- Arrow function inline (in event handlers) -->
    <button @click="() => console.log('clicked')">Click</button>
    <button @click="(e) => console.log(e.target)">With Event</button>

    <!-- Type assertions (with parentheses) -->
    <p>{{ (nullableValue as string).length }}</p>

    <!-- Destructuring in expressions -->
    <p>{{ ({ a: 1, b: 2 }).a }}</p>

    <!-- Array spread -->
    <p>{{ [...items, 'd'].join(', ') }}</p>

    <!-- Object spread -->
    <p>{{ JSON.stringify({ ...user, extra: true }) }}</p>

    <!-- Math operations -->
    <p>{{ Math.max(1, 2, 3) }}</p>
    <p>{{ Math.round(3.7) }}</p>
    <p>{{ Math.PI.toFixed(2) }}</p>

    <!-- String methods -->
    <p>{{ name.slice(0, 2) }}</p>
    <p>{{ name.split('').reverse().join('') }}</p>
    <p>{{ name.padStart(10, '-') }}</p>

    <!-- Date expressions -->
    <p>{{ now.getFullYear() }}</p>
    <p>{{ new Date().toLocaleDateString() }}</p>

    <!-- Comparison operators -->
    <p>{{ count === 1 }}</p>
    <p>{{ count !== 0 }}</p>
    <p>{{ count >= 1 && count <= 10 }}</p>

    <!-- Bitwise (rarely used but valid) -->
    <p>{{ count | 0 }}</p>
    <p>{{ count & 1 }}</p>
    <p>{{ count << 1 }}</p>

    <!-- typeof / instanceof -->
    <p>{{ typeof name }}</p>
    <p>{{ typeof count }}</p>
  </div>
</template>
