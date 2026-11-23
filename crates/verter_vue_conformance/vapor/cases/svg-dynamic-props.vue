<template>
  <svg v-bind="attrs" />
  <svg :[name]="value"><text :textContent="label" :ariaLabel="hint" :aria-label="note" /></svg>
  <symbol :innerHTML="icon" :value="level" />
</template>
