<script lang="ts">
import { defineComponent } from "vue";
const MyComp = defineComponent({
  props: {
    foo: String,
  },
});
export default {
  props: {
    foo: String,
  },
  components: {
    MyComp,
  },
};
</script>
<template>
  <!-- <MyComp :foo="1" /> -->

  <MyComp foo="dd"></MyComp>

  {{ foo }}
</template>
