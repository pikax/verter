<script lang="ts" setup>
const props = defineProps<{
  name: string;
  foo: "foo" | "bar";
}>();

const emit = defineEmits<{
  foo: [{ foo: string }];
}>();

const slots = defineSlots<{
  default(args: { name: string }): any;
}>();

props.foo === "bar";
//@ts-expect-error
props.foo === "random";

emit("foo", { foo: "" });
//@ts-expect-error
emit("foo", 1);
//@ts-expect-error
emit("random", { foo: "" });

slots.default;
slots.default({ name: "" });
// @ts-expect-error
slots.default({ random: "" });
// @ts-expect-error
slots.foo({ name: "" });
</script>
