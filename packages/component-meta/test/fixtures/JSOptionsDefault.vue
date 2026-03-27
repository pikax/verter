<script>
export default {
  props: {
    message: {
      type: String,
      default: "Hello from JS",
    },
    count: {
      type: Number,
      required: true,
    },
  },
};
</script>

<template>
  <div>{{ message }} {{ count }}</div>
</template>
