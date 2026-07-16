<script setup lang="ts">
/**
 * Fixture entry: imports the parity surface components so project graph is live.
 * Individual tests open dedicated files for focused anchors.
 */
import DailyBinding from "./DailyBinding.vue";
import DeepConsumer from "./fallthrough/DeepConsumer.vue";
import EventDeepConsumer from "./fallthrough/EventDeepConsumer.vue";
import MacroSurface from "./macros/MacroSurface.vue";
import GenericConsumer from "./generics/GenericConsumer.vue";
import GenericInferGood from "./generics/GenericInferGood.vue";
import ControlFlow from "./control/ControlFlow.vue";
import BadPropParent from "./diagnostics/BadPropParent.vue";
import NativeModel from "./features/NativeModel.vue";
import DynamicBind from "./features/DynamicBind.vue";
import ExposeParent from "./features/ExposeParent.vue";
import OptionsApi from "./features/OptionsApi.vue";
import DualScript from "./features/DualScript.vue";
import OrganizeImports from "./features/OrganizeImports.vue";
import MappingCase from "./features/MappingCase.vue";
import Narrowing from "./features/Narrowing.vue";
import ScopedStyle from "./features/ScopedStyle.vue";
import JsRename from "./features/JsRename.vue";
import Directives from "./matrix/Directives.vue";
import SlotsEmits from "./matrix/SlotsEmits.vue";
import StyleBind from "./matrix/StyleBind.vue";
import JsSurface from "./matrix/JsSurface.vue";
import TeleportSuspense from "./matrix/TeleportSuspense.vue";
import ExposePublic from "./features/ExposePublic.vue";
import IntrinsicElements from "./features/IntrinsicElements.vue";
import SlotCorrect from "./slots/SlotCorrect.vue";
import IdeSurfaceParent from "./ide/IdeSurfaceParent.vue";
</script>

<template>
  <main>
    <DailyBinding />
    <DeepConsumer />
    <EventDeepConsumer />
    <MacroSurface />
    <GenericConsumer />
    <GenericInferGood />
    <ControlFlow />
    <BadPropParent />
    <NativeModel />
    <DynamicBind />
    <ExposeParent />
    <OptionsApi title="opts" />
    <DualScript />
    <OrganizeImports />
    <MappingCase />
    <Narrowing />
    <ScopedStyle />
    <JsRename />
    <Directives />
    <SlotsEmits />
    <StyleBind />
    <JsSurface />
    <TeleportSuspense />
    <ExposePublic />
    <IntrinsicElements />
    <SlotCorrect />
    <IdeSurfaceParent />
  </main>
</template>
