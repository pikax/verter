<script lang="ts" setup>
const Supa = {
  foo: "foo",
  bar: "bar",
};
const { foo } = Supa;
</script>
<template>
  <div>
    {{ foo }}
  </div>
  <div>
    {{ Supa.foo }}
  </div>
  <div>
    <!-- @ts-expect-error -->
    {{ bar }}
  </div>
</template>
