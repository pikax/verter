/* @jsxImportSource vue */
import { ComponentInstance } from "vue";
type ExtractRenderComponents<T> = {};

import { defineComponent } from "vue";
import { unref, ComponentExpectedProps, renderList } from "vue";
import { DeclareComponent } from "vue";
import { DeclareEmits, EmitsToProps } from "vue";

const __options = defineComponent({});
type Type__options = typeof __options;

// expose

// template

function __templateResolver() {
  const ctx = () => {
    return {
      ...({} as ComponentInstance<__COMP__>),
      ...({} as ComponentExpectedProps<__COMP__>),
    };
  };

  const _ctx = ctx();
  const _comp = {} as any as ExtractRenderComponents<typeof _ctx>;

  return (
    <>
      <div>{"Test"}</div>
    </>
  );
}

type __PROPS__ = EmitsToProps<{}>;
type __DATA__ = {};
type __EMITS__ = {};
type __SLOTS__ = {};

type __COMP__ = DeclareComponent<
  __PROPS__,
  __DATA__,
  __EMITS__,
  __SLOTS__,
  "setup" extends keyof Type__options ? Type__options & { setup: () => {} } : Type__options
>;

export default __options as unknown as __COMP__;
