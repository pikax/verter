<script setup lang="ts">
import Comp from "./Comp.vue";
import CompSetup from "./Comp.setup.vue";
const c = new Comp();
const cs = new CompSetup();

c.$data.foo;
c.$props;
c.$props.bar;

cs.$data.foo;
cs.$props;
cs.$props.bar;
</script>

<template>
  <Comp bar="foo" />
  <Comp :bar="1" />

  <CompSetup />
  <CompSetup bar="foo" />
  <CompSetup :bar="1" />

  {{ $data }}
</template>
