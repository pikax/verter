// Registers GlobalEmitComp as a global component so a consumer can use <GlobalEmitComp>
// in its template without importing it. The IDE codegen reads this augmentation when
// synthesizing the `GlobalComponents` fallback const for event typing.
import GlobalEmitComp from "./GlobalEmitComp.vue";
import GlobalCountComp from "./GlobalCountComp.vue";

declare module "vue" {
  interface GlobalComponents {
    GlobalEmitComp: typeof GlobalEmitComp;
    GlobalCountComp: typeof GlobalCountComp;
  }
}

export {};
