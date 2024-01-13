import {
  DeclareComponent,
  ComponentProps,
  ComponentEmits,
  ComponentData,
  ComponentSlots,
} from "vue";

declare global {
  var __COMP__: DeclareComponent;

  function expectType<T>(value: T): void;

  // extract

  function getComponentProps<T>(component: T): ComponentProps<T>;
  function getComponentEmits<T>(component: T): ComponentEmits<T>;
  function getComponentData<T>(component: T): ComponentData<T>;
  function getComponentSlots<T>(component: T): ComponentSlots<T>;
}
