<template>
  <div v-for="user in users" :key="user.id"></div>
  <li v-for="(item, index, key) in items" :key="index"></li>
  <li v-for="{ id, name, email } in users" :key="id"></li>
  <li v-for="({ id, name, email }, index) in users" :key="id"></li>
  <li v-for="(item, index) in 8" :key="index"></li>
  <li v-for="(item, index) in [1]" :key="index"></li>
  <li v-for="n in even(numbers)"></li>

  <div v-for="user of users" :key="user.id"></div>
  <li v-for="(item, index, key) of items" :key="index"></li>
  <li v-for="{ id, name, email } of users" :key="id"></li>
  <li v-for="({ id, name, email }, index) of users" :key="id"></li>
  <li v-for="(item, index) of 8" :key="index"></li>
  <li v-for="(item, index) of [1]" :key="index"></li>
</template>
