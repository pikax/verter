/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for SlotsToRender type helper including:
 * - Basic slot props (string, number, boolean, objects)
 * - Optional and required props
 * - Multiple slots with different prop types
 * - Complex nested props and union types
 * - Edge cases (empty slots, optional slots)
 */
import "../tsx/tsx";
import { assertType, describe, it } from "vitest";
import { defineComponent, SlotsType } from "vue";
import { SlotsToRender } from "./slots";

describe("SlotsToRender", () => {
  describe("basic slot props", () => {
    describe("string props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { msg: string }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid string prop", () => {
        <>
          <RenderSlots.default msg="hello" />
          <RenderSlots.default msg="" />
          <RenderSlots.default msg={"dynamic"} />
        </>;
      });

      it("rejects missing required prop", () => {
        <>
          {/* @ts-expect-error missing required props */}
          <RenderSlots.default />
          {/* @ts-expect-error missing required props */}
          <RenderSlots.default></RenderSlots.default>
        </>;
      });

      it("rejects wrong type", () => {
        <>
          {/* @ts-expect-error wrong type - number instead of string */}
          <RenderSlots.default msg={123} />
          {/* @ts-expect-error wrong type - boolean instead of string */}
          <RenderSlots.default msg={true} />
          {/* @ts-expect-error wrong type - object instead of string */}
          <RenderSlots.default msg={{ text: "hello" }} />
        </>;
      });
    });

    describe("number props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { count: number }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid number prop", () => {
        <>
          <RenderSlots.default count={42} />
          <RenderSlots.default count={0} />
          <RenderSlots.default count={-1} />
          <RenderSlots.default count={3.14} />
        </>;
      });

      it("rejects wrong type", () => {
        <>
          {/* @ts-expect-error wrong type - string instead of number */}
          <RenderSlots.default count="42" />
          {/* @ts-expect-error wrong type - boolean instead of number */}
          <RenderSlots.default count={false} />
        </>;
      });
    });

    describe("boolean props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { active: boolean }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid boolean prop", () => {
        <>
          <RenderSlots.default active={true} />
          <RenderSlots.default active={false} />
        </>;
      });

      it("rejects wrong type", () => {
        <>
          {/* @ts-expect-error wrong type - string instead of boolean */}
          <RenderSlots.default active="true" />
          {/* @ts-expect-error wrong type - number instead of boolean */}
          <RenderSlots.default active={1} />
        </>;
      });
    });

    describe("object props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { user: { id: number; name: string } }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid object prop", () => {
        <>
          <RenderSlots.default user={{ id: 1, name: "John" }} />
        </>;
      });

      it("rejects incomplete object", () => {
        <>
          {/* @ts-expect-error missing name property */}
          <RenderSlots.default user={{ id: 1 }} />
          {/* @ts-expect-error missing id property */}
          <RenderSlots.default user={{ name: "John" }} />
          {/* @ts-expect-error empty object */}
          <RenderSlots.default user={{}} />
        </>;
      });

      it("rejects wrong object shape", () => {
        <>
          {/* @ts-expect-error wrong type for id */}
          <RenderSlots.default user={{ id: "1", name: "John" }} />
        </>;
      });
    });

    describe("array props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { items: string[] }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid array prop", () => {
        <>
          <RenderSlots.default items={["a", "b", "c"]} />
          <RenderSlots.default items={[]} />
        </>;
      });

      it("rejects wrong array element type", () => {
        <>
          {/* @ts-expect-error wrong array element type */}
          <RenderSlots.default items={[1, 2, 3]} />
          {/* @ts-expect-error mixed array types */}
          <RenderSlots.default items={["a", 1]} />
        </>;
      });
    });
  });

  describe("optional and required props", () => {
    describe("optional props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { required: string; optional?: number }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts with only required props", () => {
        <>
          <RenderSlots.default required="hello" />
        </>;
      });

      it("accepts with both required and optional props", () => {
        <>
          <RenderSlots.default required="hello" optional={42} />
        </>;
      });

      it("rejects missing required prop", () => {
        <>
          {/* @ts-expect-error missing required prop */}
          <RenderSlots.default optional={42} />
          {/* @ts-expect-error missing required prop */}
          <RenderSlots.default />
        </>;
      });
    });

    describe("all optional props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { a?: string; b?: number }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts empty props", () => {
        <>
          <RenderSlots.default />
        </>;
      });

      it("accepts partial props", () => {
        <>
          <RenderSlots.default a="hello" />
          <RenderSlots.default b={42} />
          <RenderSlots.default a="hello" b={42} />
        </>;
      });
    });

    describe("all required props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { a: string; b: number; c: boolean }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("requires all props", () => {
        <>
          <RenderSlots.default a="hello" b={42} c={true} />
        </>;
      });

      it("rejects missing any prop", () => {
        <>
          {/* @ts-expect-error missing c */}
          <RenderSlots.default a="hello" b={42} />
          {/* @ts-expect-error missing b and c */}
          <RenderSlots.default a="hello" />
          {/* @ts-expect-error missing all */}
          <RenderSlots.default />
        </>;
      });
    });
  });

  describe("multiple slots", () => {
    const Component = new (defineComponent({
      slots: {} as SlotsType<{
        default: (props: { msg: string }) => any;
        header: (props: { title: string }) => any;
        footer: (props: { year: number }) => any;
        empty: (props: Record<string, never>) => any;
      }>,
    }))();

    const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

    it("each slot has correct props", () => {
      <>
        <RenderSlots.default msg="content" />
        <RenderSlots.header title="Page Title" />
        <RenderSlots.footer year={2024} />
        <RenderSlots.empty />
      </>;
    });

    it("rejects wrong props for each slot", () => {
      <>
        {/* @ts-expect-error wrong prop name */}
        <RenderSlots.default title="content" />
        {/* @ts-expect-error wrong prop type */}
        <RenderSlots.header title={123} />
        {/* @ts-expect-error wrong prop type */}
        <RenderSlots.footer year="2024" />
        {/* @ts-expect-error unexpected prop */}
        <RenderSlots.empty msg="should not have props" />
      </>;
    });

    it("slots are independent", () => {
      <>
        {/* @ts-expect-error mixing props from different slots */}
        <RenderSlots.default title="wrong" year={2024} />
      </>;
    });
  });

  describe("slots without props", () => {
    const Component = new (defineComponent({
      slots: {} as SlotsType<{
        // Slots without props should use empty object type for proper JSX compatibility
        default: (props: Record<string, never>) => any;
        header: (props: Record<string, never>) => any;
      }>,
    }))();

    const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

    it("accepts no props", () => {
      <>
        <RenderSlots.default />
        <RenderSlots.header />
        <RenderSlots.default></RenderSlots.default>
      </>;
    });
  });

  describe("complex prop types", () => {
    describe("union types", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { value: string | number }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts any union member", () => {
        <>
          <RenderSlots.default value="hello" />
          <RenderSlots.default value={42} />
        </>;
      });

      it("rejects non-union types", () => {
        <>
          {/* @ts-expect-error boolean not in union */}
          <RenderSlots.default value={true} />
          {/* @ts-expect-error object not in union */}
          <RenderSlots.default value={{ text: "hello" }} />
        </>;
      });
    });

    describe("literal types", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: {
            status: "active" | "inactive" | "pending";
          }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid literals", () => {
        <>
          <RenderSlots.default status="active" />
          <RenderSlots.default status="inactive" />
          <RenderSlots.default status="pending" />
        </>;
      });

      it("rejects invalid literals", () => {
        <>
          {/* @ts-expect-error invalid literal */}
          <RenderSlots.default status="unknown" />
          {/* @ts-expect-error wrong type */}
          <RenderSlots.default status={1} />
        </>;
      });
    });

    describe("nested complex types", () => {
      type User = {
        id: number;
        profile: {
          name: string;
          email: string;
          settings: {
            theme: "light" | "dark";
            notifications: boolean;
          };
        };
        roles: ("admin" | "user" | "guest")[];
      };

      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { user: User }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid nested structure", () => {
        <>
          <RenderSlots.default
            user={{
              id: 1,
              profile: {
                name: "John",
                email: "john@example.com",
                settings: {
                  theme: "dark",
                  notifications: true,
                },
              },
              roles: ["admin", "user"],
            }}
          />
        </>;
      });

      it("rejects invalid nested structure", () => {
        <>
          <RenderSlots.default
            user={{
              id: 1,
              profile: {
                name: "John",
                email: "john@example.com",
                settings: {
                  // @ts-expect-error invalid theme value - "blue" is not "light" | "dark"
                  theme: "blue",
                  notifications: true,
                },
              },
              roles: ["admin"],
            }}
          />
        </>;
      });
    });

    describe("function props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: {
            onClick: (id: number) => void;
            onData: (data: { value: string }) => Promise<void>;
          }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts valid function props", () => {
        <>
          <RenderSlots.default
            onClick={(id) => console.log(id)}
            onData={async (data) => console.log(data.value)}
          />
        </>;
      });

      it("rejects wrong function signature", () => {
        <>
          <RenderSlots.default
            // @ts-expect-error wrong parameter type - expects (id: number) => void
            onClick={(id: string) => console.log(id)}
            onData={async (data) => console.log(data.value)}
          />
        </>;
      });
    });

    describe("generic-like patterns", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: {
            data: unknown;
            render: (item: unknown) => any;
          }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts unknown type", () => {
        <>
          <RenderSlots.default data="string" render={(item) => item} />
          <RenderSlots.default data={123} render={(item) => item} />
          <RenderSlots.default
            data={{ complex: true }}
            render={(item) => item}
          />
        </>;
      });
    });
  });

  describe("optional slots", () => {
    const Component = new (defineComponent({
      slots: {} as SlotsType<{
        default: (props: { msg: string }) => any;
        header?: (props: { title: string }) => any;
        footer?: (props: Record<string, never>) => any;
      }>,
    }))();

    const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

    it("required slot works normally", () => {
      <>
        <RenderSlots.default msg="hello" />
      </>;
    });

    it("optional slots type reflects optionality", () => {
      // Optional slots in SlotsType result in optional keys in $slots
      // SlotsToRender maps these but they may be undefined
      type RenderType = typeof RenderSlots;
      type HeaderType = RenderType["header"];
      type FooterType = RenderType["footer"];

      // These types exist and can be assigned (undefined is part of the union)
      const _header: HeaderType = undefined;
      const _footer: FooterType = undefined;
      void _header;
      void _footer;
    });
  });

  describe("edge cases", () => {
    describe("empty props object", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: Record<string, never>) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts empty object", () => {
        <>
          <RenderSlots.default />
        </>;
      });
    });

    describe("void return type", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { msg: string }) => void;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("works with void return", () => {
        <>
          <RenderSlots.default msg="hello" />
        </>;
      });
    });

    describe("never return type", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { msg: string }) => never;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("works with never return", () => {
        <>
          <RenderSlots.default msg="hello" />
        </>;
      });
    });

    describe("tuple return type", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { msg: string }) => [string, number];
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("works with tuple return", () => {
        <>
          <RenderSlots.default msg="hello" />
        </>;
      });
    });

    describe("slot with 'any' props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          default: (props: any) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts any props", () => {
        <>
          <RenderSlots.default />
          <RenderSlots.default anything="goes" />
          <RenderSlots.default num={123} bool={true} obj={{ a: 1 }} />
        </>;
      });
    });

    describe("slot with index signature", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { [key: string]: string }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Component.$slots>;

      it("accepts string indexed props", () => {
        <>
          <RenderSlots.default />
          <RenderSlots.default foo="bar" />
          <RenderSlots.default a="1" b="2" c="3" />
        </>;
      });

      it("rejects non-string values", () => {
        <>
          {/* @ts-expect-error value must be string */}
          <RenderSlots.default foo={123} />
        </>;
      });
    });
  });

  describe("real-world patterns", () => {
    describe("data table slot", () => {
      type Column<T> = {
        key: keyof T;
        label: string;
      };

      type TableItem = {
        id: number;
        name: string;
        email: string;
      };

      const DataTable = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: {
            item: TableItem;
            index: number;
            column: Column<TableItem>;
          }) => any;
          header: (props: { columns: Column<TableItem>[] }) => any;
          empty: (props: Record<string, never>) => any;
          loading: (props: { progress: number }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof DataTable.$slots>;

      it("renders data table slots correctly", () => {
        <>
          <RenderSlots.default
            item={{ id: 1, name: "John", email: "john@test.com" }}
            index={0}
            column={{ key: "name", label: "Name" }}
          />
          <RenderSlots.header
            columns={[
              { key: "id", label: "ID" },
              { key: "name", label: "Name" },
            ]}
          />
          <RenderSlots.empty />
          <RenderSlots.loading progress={50} />
        </>;
      });
    });

    describe("modal slot", () => {
      const Modal = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { close: () => void }) => any;
          title: (props: { text: string }) => any;
          footer: (props: {
            confirm: () => void;
            cancel: () => void;
            isLoading: boolean;
          }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Modal.$slots>;

      it("renders modal slots correctly", () => {
        <>
          <RenderSlots.default close={() => {}} />
          <RenderSlots.title text="Confirm Action" />
          <RenderSlots.footer
            confirm={() => {}}
            cancel={() => {}}
            isLoading={false}
          />
        </>;
      });
    });

    describe("form field slot", () => {
      const FormField = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: {
            value: string;
            onChange: (value: string) => void;
            error: string | null;
            touched: boolean;
          }) => any;
          label: (props: { required: boolean }) => any;
          error: (props: { message: string }) => any;
          hint: (props: Record<string, never>) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof FormField.$slots>;

      it("renders form field slots correctly", () => {
        <>
          <RenderSlots.default
            value=""
            onChange={(v) => console.log(v)}
            error={null}
            touched={false}
          />
          <RenderSlots.label required={true} />
          <RenderSlots.error message="This field is required" />
          <RenderSlots.hint />
        </>;
      });
    });

    describe("tabs component slot", () => {
      type Tab = {
        id: string;
        label: string;
        disabled?: boolean;
      };

      const Tabs = new (defineComponent({
        slots: {} as SlotsType<{
          tab: (props: { tab: Tab; isActive: boolean; index: number }) => any;
          panel: (props: { tab: Tab }) => any;
          default: (props: { activeTab: Tab | null }) => any;
        }>,
      }))();

      const RenderSlots = {} as SlotsToRender<typeof Tabs.$slots>;

      it("renders tabs slots correctly", () => {
        const tab: Tab = { id: "1", label: "Tab 1" };
        <>
          <RenderSlots.tab tab={tab} isActive={true} index={0} />
          <RenderSlots.panel tab={tab} />
          <RenderSlots.default activeTab={tab} />
          <RenderSlots.default activeTab={null} />
        </>;
      });
    });
  });

  describe("type assertions", () => {
    it("SlotsToRender produces correct component type", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { msg: string }) => any;
        }>,
      }))();

      type Slots = SlotsToRender<typeof Component.$slots>;

      // Assert the structure of the resulting type
      assertType<Slots["default"]>(
        {} as { new (): { $props: { msg: string } } }
      );
    });

    it("preserves optional props in $props", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: (props: { required: string; optional?: number }) => any;
        }>,
      }))();

      type Slots = SlotsToRender<typeof Component.$slots>;
      type DefaultProps = InstanceType<Slots["default"]>["$props"];

      assertType<DefaultProps>({ required: "test" });
      assertType<DefaultProps>({ required: "test", optional: 42 });
    });

    it("handles slots without props correctly", () => {
      const Component = new (defineComponent({
        slots: {} as SlotsType<{
          default: () => any;
        }>,
      }))();

      type Slots = SlotsToRender<typeof Component.$slots>;

      // For slots without props (no parameter), the inferred P is `unknown`
      // This results in { new(): { $props: unknown } }
      assertType<Slots["default"]>({} as { new (): { $props: unknown } });
    });

    it("non-function slot produces fallback type", () => {
      type NonFunctionSlots = {
        default: string;
        header: number;
      };

      type Result = SlotsToRender<NonFunctionSlots>;

      // Non-function slots don't match the conditional, so they become () => any
      const defaultSlot: Result["default"] = () => "anything";
      const headerSlot: Result["header"] = () => 123;
      assertType<() => any>(defaultSlot);
      assertType<() => any>(headerSlot);
    });
  });
});
