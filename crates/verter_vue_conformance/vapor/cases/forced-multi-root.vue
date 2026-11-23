<template>
  <ul>
    <template v-for="item in items"><li v-if="item.open">{{ item.name }}</li></template>
    <template v-for="item in rows"><li v-if="item.a">a</li><li v-else-if="item.b">b</li><li v-else>c</li></template>
  </ul>
</template>
