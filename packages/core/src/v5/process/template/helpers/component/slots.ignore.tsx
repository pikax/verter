import { defineComponent, SlotsType } from "vue";
import { $V_StrictRenderSlot } from "./slots.js";

const Comp = defineComponent({
  props: {
    foo: String,
    type: String,
  },

  slots: Object as SlotsType<{
    default: () => any[];
    foo: (props: { foo: string }) => any[];
    single: (props: { type: string }) => HTMLInputElement;
    multiple: (props: { type: string }) => HTMLInputElement[];

    two: (props: { type: string }) => [HTMLMediaElement, HTMLAnchorElement];
  }>,
});

// any
$V_StrictRenderSlot(new Comp().$slots.default, [null]);

// foo
$V_StrictRenderSlot(new Comp().$slots.foo, [{ foo: "foo" }]);

// single
$V_StrictRenderSlot(new Comp().$slots.single, [new HTMLInputElement()]);

// @ts-expect-error
$V_StrictRenderSlot(new Comp().$slots.single, [
  new HTMLInputElement(),
  new HTMLInputElement(),
]);

// multiple
$V_StrictRenderSlot(new Comp().$slots.multiple, [new HTMLInputElement()]);

// @ts-expect-error
$V_StrictRenderSlot(new Comp().$slots.multiple, [new HTMLElement()]);

// two
$V_StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  new HTMLAnchorElement(),
]);

// @ts-expect-error
$V_StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  new HTMLAnchorElement(),
  new HTMLAnchorElement(),
]);

$V_StrictRenderSlot(new Comp().$slots.two, [
  new HTMLMediaElement(),
  // @ts-expect-error
  new HTMLMediaElement(),
]);

// <Comp>
//       <template #default="{ value, type }">
//         <div v-if="value">
//           <a :href="\`tel:\${value}\`"> {{ value }}</a> <span> ({{ type }})</span>
//         </div>
//         <span v-else>—</span>
//       </template></Comp>

<>
  <Comp
    v-slot={({ $slots }) => {
      <input
        v-slot={(child1) => {
          $V_StrictRenderSlot($slots.single, [child1]);
        }}
      />;
    }}
  />
</>;

declare const SupaComp: {
  new <T>(): {
    $props: {
      foo: T;
      onFoo: (e: { bar: "bar"; foo: T }) => any;
    };
  };
};

<>
  <SupaComp ref="el" foo={"hello"} onFoo={(e) => e.bar && e.foo} />
  <SupaComp ref="el1" foo={{ bax: 1 }} onFoo={(e) => e.bar && e.foo.bax} />
  {1 === 1 /* fake condition for the sake of the example */ ? (
    <SupaComp ref="el2" foo={42} onFoo={(e) => e.bar && e.foo} />
  ) : (
    <SupaComp ref="el2" foo={"42"} onFoo={(e) => e.bar && e.foo} />
  )}
</>;

// I want to retrieve the type for SupaComp 1 & 2