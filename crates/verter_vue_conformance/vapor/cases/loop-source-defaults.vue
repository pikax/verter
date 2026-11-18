<template><div v-for="x in ((list, copy = list) => copy)([1])">{{ x }}</div><div v-for="x in (function(list, copy = list) { return copy })([1])">{{ x }}</div></template>
