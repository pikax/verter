<script setup lang="ts">
import { ref, watch, watchEffect, watchPostEffect, watchSyncEffect } from "vue";

const count = ref(0);
const name = ref("John");
const user = ref({ name: "John", age: 30 });

// Basic watch with typed callback
watch(count, (newVal, oldVal) => {
  const _new: number = newVal;
  const _old: number = oldVal;
});

// Watch with immediate and deep options
watch(
  user,
  (newVal, oldVal, onCleanup) => {
    const _name: string = newVal.name;
    onCleanup(() => {
      // cleanup logic
    });
  },
  { immediate: true, deep: true }
);

// Watch multiple sources (tuple)
watch([count, name] as const, ([newCount, newName], [oldCount, oldName]) => {
  const _nc: number = newCount;
  const _nn: string = newName;
  const _oc: number = oldCount;
  const _on: string = oldName;
});

// Watch getter function
watch(
  () => user.value.age,
  (newAge, oldAge) => {
    const _new: number = newAge;
    const _old: number = oldAge;
  }
);

// Watch multiple getters
watch(
  [() => user.value.name, () => user.value.age] as const,
  ([newName, newAge]) => {
    const _name: string = newName;
    const _age: number = newAge;
  }
);

// watchEffect - auto tracks dependencies
watchEffect((onCleanup) => {
  console.log("Count is:", count.value);
  onCleanup(() => {
    // cleanup
  });
});

// watchEffect with flush option
watchEffect(
  () => {
    console.log("Name is:", name.value);
  },
  { flush: "post" }
);

// watchPostEffect - shorthand for flush: 'post'
watchPostEffect(() => {
  console.log("Post effect, count:", count.value);
});

// watchSyncEffect - shorthand for flush: 'sync'
watchSyncEffect(() => {
  console.log("Sync effect, count:", count.value);
});

// Watch with once option (Vue 3.4+)
watch(
  count,
  (newVal) => {
    console.log("Triggered once:", newVal);
  },
  { once: true }
);

// Stop handle
const stop = watch(count, () => {});
stop(); // Stop watching
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>Name: {{ name }}</p>
  </div>
</template>
