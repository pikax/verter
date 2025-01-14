import 'vue/jsx'
import { $V_NormaliseComponentKey } from "./name";

function normaliseComponents<T extends Record<PropertyKey, any>>(
  obj: T
): {
  [K in keyof T as $V_NormaliseComponentKey<K & string>]: T[K];
} {
  return {} as any;
}

const helloFoo = {};
const HelloMotto = {};

const CTX_Components = normaliseComponents({
  // ...{} as GlobalComponents,
  helloFoo,
  HelloMotto,
  HelloMoto: {},

  snake_foo: {},
  snake_fooBar: {},
});

CTX_Components.HelloMotto;
CTX_Components["hello-motto"];
CTX_Components.helloMotto;
CTX_Components["Hello-Motto"];
// @ts-expect-error
CTX_Components.hellomotto;

CTX_Components["hello-foo"];
CTX_Components["helloFoo"];
CTX_Components["hello-Foo"];
// @ts-expect-error
CTX_Components["Hello-Foo"];

CTX_Components["snake_foo"];
//@ts-expect-error
CTX_Components.snakeFoo;

CTX_Components.snake_fooBar;
CTX_Components["snake_foo-bar"];
CTX_Components["snake_foo-Bar"];
//@ts-expect-error
CTX_Components.snakeFooBar;
// @ts-expect-error
CTX_Components["snake-foo-bar"];
