import { defineComponent } from "vue";
import Comp from "./Comp.vue";

const t = new Comp();
t.$data.foo;

declare const PublicInstance: InstanceType<typeof Comp>;

PublicInstance.$data.foo;

// const Foo;
const Foo = defineComponent({
  props: undefined,
  data() {
    return {};
  },
});

const f = new Foo();

const Parent = defineComponent({
  components: {
    Foo,
  },
});

const ctx = {
  ...({} as Required<(typeof Comp)["components"]>),
};

ctx.Child;
