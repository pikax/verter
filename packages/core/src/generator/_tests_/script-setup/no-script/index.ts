import { defineComponent } from "vue";

type __COMP__ = typeof ComponentOptions;
 const ComponentOptions = defineComponent({ });
declare const Comp: __COMP__;


expectType<__COMP__>(Comp);
