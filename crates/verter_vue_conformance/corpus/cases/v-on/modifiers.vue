<script setup>
function save() {}
function del() {}
function open() {}
</script>

<template>
  <div @click.self="save">
    <button @click.stop.prevent="del">Delete</button>
    <a href="/x" @click.once.capture="open">Open</a>
  </div>
</template>
