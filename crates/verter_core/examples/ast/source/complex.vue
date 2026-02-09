<template>
  <div id="app" class="container">
    <h1>{{ title }}</h1>
    <p v-if="showMessage">{{ message }}</p>
    <button @click="handleClick" :disabled="isDisabled">
      Click me
    </button>
    <ul>
      <li v-for="item in items" :key="item.id">{{ item.name }}</li>
      <li v-for="  item in items   " :key="item.id">{{ item.name }}</li>

      <li v-for="item of items" :key="item.id">{{ item.name }}</li>
    </ul>
    <!-- This is a comment -->
    <input type="text" v-model="inputValue" />
    <CustomComponent :prop="value" @event="handler" />
  </div>
</template>
