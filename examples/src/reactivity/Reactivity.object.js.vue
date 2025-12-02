<script>
export default {
  name: "Reactivity",
  data() {
    return {
      count: 0,
      name: "John",
    };
  },
  computed: {
    doubleCount() {
      return this.count * 2;
    },
    greeting() {
      return `Hello, ${this.name}!`;
    },
  },
  methods: {
    increment() {
      this.count++;
    },
    updateName(newName) {
      this.name = newName;
    },
  },
};
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>Double: {{ doubleCount }}</p>
    <p>{{ greeting }}</p>
    <button @click="increment">Increment</button>
    <input :value="name" @input="(e) => updateName(e.target.value)" />
  </div>
</template>
