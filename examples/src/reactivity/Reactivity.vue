<script setup lang="ts">
import { ref, reactive, shallowRef, shallowReactive, toRef, toRefs, toRaw, markRaw, readonly, shallowReadonly } from "vue";

// ref - deep reactive reference
const count = ref(0);
const nested = ref({
  deep: {
    value: 1,
  },
});

// shallowRef - only .value is reactive
const shallowNested = shallowRef({
  deep: {
    value: 1,
  },
});

// reactive - deep reactive object
const state = reactive({
  count: 0,
  nested: {
    deep: {
      value: 1,
    },
  },
});

// shallowReactive - only top-level properties are reactive
const shallowState = shallowReactive({
  count: 0,
  nested: {
    deep: {
      value: 1,
    },
  },
});

// toRef - create ref from reactive property
const stateCountRef = toRef(state, "count");

// toRef with getter (Vue 3.3+)
const doubleCount = toRef(() => state.count * 2);

// toRefs - destructure reactive object to refs
const { count: countRef, nested: nestedRef } = toRefs(state);

// toRaw - get raw object from reactive
const rawState = toRaw(state);

// markRaw - mark object as non-reactive
const nonReactive = markRaw({
  value: 1,
});
const stateWithRaw = reactive({
  data: nonReactive, // data will not be made reactive
});

// readonly - deep readonly reactive
const readonlyState = readonly(state);

// shallowReadonly - only top-level is readonly
const shallowReadonlyState = shallowReadonly(state);

// Props with toRefs pattern
const props = defineProps<{
  initialCount: number;
  config: { threshold: number };
}>();

// Create refs from props (maintains reactivity)
const { initialCount, config } = toRefs(props);
const thresholdRef = toRef(props.config, "threshold");
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>State count: {{ state.count }}</p>
    <p>Initial count prop: {{ initialCount }}</p>
  </div>
</template>
