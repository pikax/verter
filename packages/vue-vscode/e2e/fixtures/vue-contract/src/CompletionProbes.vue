<script setup lang="ts">
import DirectChild from "./components/DirectChild.vue";
import RegionSlotHost from "./RegionSlotHost.vue";

interface ProbeShape {
  probeLabel: string;
  probeCount: number;
}

const probeValue: ProbeShape = { probeLabel: "probe", probeCount: 1 };
const probeDynamic = RegionSlotHost;
// The plain-script control: member completion here must be unaffected by anything a
// template-region completion source does.
const probeMember = probeValue.probeLabel;
void probeMember;
</script>

<template>
  <section>
    <!--
      Each probe element carries a unique `data-probe` marker and exactly ONE space
      before its self-closing slash. A completion probe inserts the trigger character
      into that space — the document state a user reaches by typing it — so the
      completion request runs against real post-typing text, not a prepared offset.
    -->
    <RegionSlotHost data-probe="component-attr" />
    <RegionSlotHost data-probe="component-event" />
    <RegionSlotHost data-probe="component-slot" />
    <RegionSlotHost data-probe="component-directive" />
    <article data-probe="intrinsic-attr" />
    <article data-probe="intrinsic-event" />
    <DirectChild data-probe="member-in-directive" :contract-prop="probeValue.probeLabel" />
    <RegionSlotHost data-probe="slot-scope" v-slot="{ hostDatum }">{{ hostDatum }}</RegionSlotHost>
    <component data-probe="dynamic-is" :is="probeDynamic" />
    <p>{{ probeValue.probeLabel }}</p>
  </section>
</template>
