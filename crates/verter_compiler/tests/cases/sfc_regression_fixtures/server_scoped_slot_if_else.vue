<script setup lang="ts">
import Child from './Child.vue';
</script>
<template>
  <Child><template #cell="{ row }"><span v-if="row.a">a</span><template v-else>{{ row.b }}</template></template></Child>
</template>
