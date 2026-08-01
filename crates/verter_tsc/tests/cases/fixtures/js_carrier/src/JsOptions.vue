<script>
export default {
  props: {
    label: { type: String, required: true },
  },
  data() {
    return { count: 0 };
  },
  methods: {
    bump(step) {
      this.count += step;
    },
    describe(item) {
      return `${this.label}: ${item.name}`;
    },
  },
};
</script>
<template>
  <button @click="bump(1)">{{ label }} {{ count }}</button>
</template>
