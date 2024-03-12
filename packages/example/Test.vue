<template>
  <div>
    <!-- this is a comment :) 
    
        Thanks x) 
    -->
    <div></div>
    <!-- @ts-expect-error -->
    <div @click="(test) => test.xx"></div>
  </div>
</template>
