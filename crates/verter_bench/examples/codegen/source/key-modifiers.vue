<template>
  <div>
    <!-- Basic key modifiers -->
    <input @keyup.enter="submitEnter" placeholder="Press Enter" />
    <input @keyup.tab="handleTab" placeholder="Press Tab" />
    <input @keyup.delete="handleDelete" placeholder="Press Delete/Backspace" />
    <input @keyup.esc="handleEsc" placeholder="Press Escape" />
    <input @keyup.space="handleSpace" placeholder="Press Space" />

    <!-- Arrow keys -->
    <input @keyup.up="handleUp" placeholder="Press Up" />
    <input @keyup.down="handleDown" placeholder="Press Down" />
    <input @keyup.left="handleLeft" placeholder="Press Left" />
    <input @keyup.right="handleRight" placeholder="Press Right" />

    <!-- System modifiers -->
    <input @keyup.ctrl.enter="submitCtrlEnter" placeholder="Ctrl+Enter" />
    <input @keyup.alt.s="saveAltS" placeholder="Alt+S" />
    <input @keyup.shift.tab="prevShiftTab" placeholder="Shift+Tab" />
    <input @keyup.meta.k="searchMetaK" placeholder="Meta+K (Cmd/Win)" />

    <!-- Exact modifier -->
    <button @click.ctrl.exact="onCtrlClick">Ctrl+Click only (no other keys)</button>
    <button @click.exact="onExactClick">Click only (no modifiers)</button>
  </div>
</template>

<script setup>
const submitEnter = () => console.log("enter");
const handleTab = () => console.log("tab");
const handleDelete = () => console.log("delete");
const handleEsc = () => console.log("esc");
const handleSpace = () => console.log("space");
const handleUp = () => console.log("up");
const handleDown = () => console.log("down");
const handleLeft = () => console.log("left");
const handleRight = () => console.log("right");
const submitCtrlEnter = () => console.log("ctrl+enter");
const saveAltS = () => console.log("alt+s");
const prevShiftTab = () => console.log("shift+tab");
const searchMetaK = () => console.log("meta+k");
const onCtrlClick = () => console.log("ctrl+click exact");
const onExactClick = () => console.log("exact click");
</script>
