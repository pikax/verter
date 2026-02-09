<template>
  <div>
    <!-- Basic modifiers -->
    <button @click.stop="handleStop">Stop Propagation</button>
    <button @click.prevent="handlePrevent">Prevent Default</button>
    <button @click.self="handleSelf">Self Only</button>

    <!-- Special naming modifiers (become part of event name) -->
    <button @click.capture="handleCapture">Capture Phase</button>
    <button @click.once="handleOnce">Once Only</button>
    <div @scroll.passive="handleScroll">Passive Scroll</div>

    <!-- Combined modifiers -->
    <button @click.stop.prevent="handleStopPrevent">Stop + Prevent</button>
    <a @click.stop.prevent="handleLink" href="#">Link with modifiers</a>

    <!-- Capture + Once combined -->
    <button @click.capture.once="handleCaptureOnce">Capture Once</button>
  </div>
</template>

<script setup>
const handleStop = () => console.log("stop");
const handlePrevent = () => console.log("prevent");
const handleSelf = () => console.log("self");
const handleCapture = () => console.log("capture");
const handleOnce = () => console.log("once");
const handleScroll = () => console.log("scroll");
const handleStopPrevent = () => console.log("stop+prevent");
const handleLink = () => console.log("link");
const handleCaptureOnce = () => console.log("capture+once");
</script>
