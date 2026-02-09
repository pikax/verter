<template>
  <div v-show="visible" v-once>
    <span v-text="textContent"></span>
    <span v-html="htmlContent"></span>
    <div v-pre>{{ this will not be compiled }}</div>
    <div v-cloak>Loading...</div>
    <div v-memo="[item]">Memoized content</div>

    <div
      @click="
        (e) => {
          let a = 1;

          console.log(a);

          return a;
        }
      "
    ></div>
  </div>
</template>
