export type MakePublicProps<T> = T extends import("vue").DefineProps<
  infer P,
  infer K extends keyof P
>
  ? {
      T: T;
      P: K;
    }
  : T;

import { defineProps, withDefaults } from "vue";
const a = {} as MakePublicProps<{ a: 1 }>;

const p = defineProps({
  f: String,
  d: {
    type: String,
    default: "bar",
  },
});

{
  <string>p.f;
  <undefined>p.f;
  // @ts-expect-error readonly
  p.f = "";
  // @ts-expect-error cast to number
  <number>p.f;

  <string>p.d;
  // @ts-expect-error not undefined
  <undefined>p.d;
  // @ts-expect-error readonly
  p.d = "";
  // @ts-expect-error cast to number
  <number>p.d;
}

const pa = {} as MakePublicProps<typeof p>;
{
  <string>pa.f;
  <undefined>pa.f;

  // @ts-expect-error readonly
  pa.f = "";
  // @ts-expect-error cast to number
  <number>pa.f;

  <string>pa.d;
  <undefined>pa.d;
  // @ts-expect-error readonly
  pa.d = "";
  // @ts-expect-error cast to number
  <number>pa.d;
}
