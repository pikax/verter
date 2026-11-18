<template>
  <Panel>
    <template v-if="open" #head><p>{{ title }}</p></template>
    <template v-else #foot>closed</template>
  </Panel>
</template>
