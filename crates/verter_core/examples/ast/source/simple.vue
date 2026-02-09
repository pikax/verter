<template>
<div class="hello" v-if="show">
  {{ message }}
  <span @click:foo.bar="onClick">text</span>
</div>
</template>
