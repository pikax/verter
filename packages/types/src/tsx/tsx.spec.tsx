import { assertType, describe, expect, it } from "vitest";
import { defineComponent, SlotsType, ref } from "vue";
import "./tsx";

describe("TSX type augmentations", () => {
  describe("v-slot on HTML elements", () => {
    it("supports v-slot on div elements", () => {
      <div
        v-slot={(el) => {
          assertType<HTMLElement>(el);
        }}
      />;
    });
    it("supports v-slot on input elements", () => {
      <input
        v-slot={(el) => {
          assertType<HTMLInputElement>(el);
          assertType<string>(el.value);
        }}
      />;
    });
    it("supports v-slot on select elements", () => {
      <select
        v-slot={(el) => {
          assertType<HTMLSelectElement>(el);
          assertType<number>(el.selectedIndex);
        }}
      />;
    });
  });

  describe("v-slot on Vue components", () => {
    const TabItem = defineComponent({
      props: { id: { type: String, required: true } },
    });
    const Tabs = defineComponent({
      slots: {} as SlotsType<{
        default: (arg: { foo: 1 }) => (typeof TabItem)[];
        header: () => string;
      }>,
      setup() {
        return { tabsId: "tabs-123" };
      },
    });
    it("infers component instance type from v-slot function parameter", () => {
      <Tabs
        v-slot={(c) => {
          assertType<string>(c.tabsId);
          c.$slots.default({ foo: 1 });
          // @ts-expect-error missing property
          c.$slots.default();
          c.$slots.header();
          // @ts-expect-error wrong arg
          c.$slots.header({ foo: 1 });
          return {} as any;
        }}
      />;
    });

    describe("generic v-slot usage", () => {
      it("string literal", () => {
        function Wrapper() {
          return Tabs as typeof Tabs & {
            new <T extends string>(): {
              $props: {
                value: T;
                onChange: (val: T) => void;
              };
            };
          };
        }
        const TabGeneric = Wrapper();
        <TabGeneric
          value={"foo"}
          onChange={(e) => {
            assertType<"foo">(e);
          }}
        />;

        <TabGeneric
          // @ts-expect-error not literal
          value={1}
          onChange={(e) => {
            assertType<string>(e);
          }}
        />;
      });

      it("string", () => {
        function Wrapper() {
          return Tabs as typeof Tabs & {
            new <T>(): {
              $props: {
                value: T;
                onChange: (val: T) => void;
              };
            };
          };
        }
        const TabGeneric = Wrapper();
        <TabGeneric
          value={"foo"}
          onChange={(e) => {
            assertType<string>(e);
            // @ts-expect-error not literal
            assertType<"foo">(e);
          }}
        />;
      });

      it("object", () => {
        function Wrapper() {
          return Tabs as typeof Tabs & {
            new <T>(): {
              $props: {
                value: T;
                onChange: (val: T) => void;
              };
            };
          };
        }
        const TabGeneric = Wrapper();
        <TabGeneric
          value={{ a: 1 }}
          onChange={(e) => {
            assertType<{ a: number }>(e);
            // @ts-expect-error not literal
            assertType<{ a: string }>(e);
          }}
        />;
      });
      it("generic with constraint", () => {
        function Wrapper() {
          return Tabs as typeof Tabs & {
            new <T extends { a: number }>(): {
              $props: {
                value: T;
                onChange: (val: T) => void;
              };
            };
          };
        }
        const TabGeneric = Wrapper();
        <TabGeneric
          value={{ a: 1, b: 1 }}
          onChange={(e) => {
            assertType<{ a: number; b: number }>(e);
            // @ts-expect-error not correct type
            assertType<{ a: number; b: string }>(e);
          }}
        />;

        <TabGeneric
          // @ts-expect-error missing property 'a'
          value={{ b: 2 }}
          onChange={(e) => {
            assertType<{ a: number }>(e);
          }}
        />;
      });

      it(" with generic", () => {
        function t<Foo extends { g: string }>(e: Foo) {
          function Wrapper() {
            return Tabs as typeof Tabs & {
              new <T extends { a: Foo }>(): {
                $props: {
                  value: T;
                  onChange: (val: T) => void;
                };
              };
            };
          }
          const TabGeneric = Wrapper();
          <TabGeneric
            value={{ a: e }}
            onChange={(e) => {
              assertType<{ a: Foo }>(e);
              // @ts-expect-error not correct type
              assertType<{ a: number; b: string }>(e);
            }}
          />;

          <TabGeneric
            // @ts-expect-error missing property 'a'
            value={{ b: 2 }}
            onChange={(e) => {
              assertType<{ a: Foo }>(e);
            }}
          />;
        }
      });
    });
  });

  describe("v-slot advanced usage", () => {
    describe("v-slot returning slots object", () => {
      it("allows returning slots object from v-slot callback", () => {
        const Tabs = defineComponent({
          slots: {} as SlotsType<{
            default: (arg: { activeTab: string }) => any;
            header: () => any;
          }>,
          setup() {
            return { tabsId: "tabs-123" };
          },
        });
        <Tabs
          v-slot={(c) => {
            assertType<string>(c.tabsId);
            return {
              default: ({ activeTab }) => <div>Active: {activeTab}</div>,
              header: () => <div>Tabs Header</div>,
            };
          }}
        />;
      });
      it("v-slot callback receives component instance with $slots", () => {
        const Container = defineComponent({
          slots: {} as SlotsType<{
            content: (props: { value: number }) => any;
          }>,
          setup() {
            return { containerId: "container-1" };
          },
        });
        <Container
          v-slot={(instance) => {
            assertType<string>(instance.containerId);
            assertType<(props: { value: number }) => any>(
              instance.$slots.content
            );
            return { content: ({ value }) => <span>{value}</span> };
          }}
        />;
      });
      it("v-slot with multiple named slots", () => {
        const Layout = defineComponent({
          slots: {} as SlotsType<{
            header: (props: { title: string }) => any;
            default: () => any;
            footer: (props: { year: number }) => any;
            sidebar: (props: { width: string }) => any;
          }>,
          setup() {
            return { layoutId: "main-layout" };
          },
        });
        <Layout
          v-slot={(c) => {
            assertType<string>(c.layoutId);
            return {
              header: ({ title }) => <h1>{title}</h1>,
              default: () => <main>Content</main>,
              footer: ({ year }) => <footer>{year}</footer>,
              sidebar: ({ width }) => <aside style={{ width }}>Sidebar</aside>,
            };
          }}
        />;
      });
      it("v-slot with optional slots", () => {
        const Panel = defineComponent({
          slots: {} as SlotsType<{
            header?: () => any;
            default: () => any;
            footer?: () => any;
          }>,
        });
        <Panel
          v-slot={() => {
            return { default: () => <div>Content</div> };
          }}
        />;
        <Panel
          v-slot={() => {
            return {
              header: () => <div>Header</div>,
              default: () => <div>Content</div>,
              footer: () => <div>Footer</div>,
            };
          }}
        />;
      });
    });
    describe("v-slot with scoped slots", () => {
      it("properly types scoped slot parameters", () => {
        const DataTable = defineComponent({
          slots: {} as SlotsType<{
            item: (props: {
              item: { id: number; name: string };
              index: number;
            }) => any;
            empty: () => any;
          }>,
        });
        <DataTable
          v-slot={() => {
            return {
              item: ({ item, index }) => {
                assertType<{ id: number; name: string }>(item);
                assertType<number>(index);
                return (
                  <div>
                    {index}: {item.name}
                  </div>
                );
              },
              empty: () => <div>No items</div>,
            };
          }}
        />;
      });
      it("v-slot with complex slot parameter types", () => {
        type User = { id: string; email: string; role: "admin" | "user" };
        const UserList = defineComponent({
          slots: {} as SlotsType<{
            user: (props: { user: User; actions: { edit: () => void } }) => any;
          }>,
        });
        <UserList
          v-slot={() => {
            return {
              user: ({ user, actions }) => {
                assertType<User>(user);
                assertType<{ edit: () => void }>(actions);
                return (
                  <div>
                    {user.email} - {user.role}
                    <button onClick={actions.edit}>Edit</button>
                  </div>
                );
              },
            };
          }}
        />;
      });
    });
    describe("v-slot type validation", () => {
      it("validates slot return types match expected slots", () => {
        const Tabs = defineComponent({
          slots: {} as SlotsType<{ tab: (props: { id: string }) => any }>,
        });
        <Tabs
          v-slot={() => {
            return { tab: ({ id }) => <div>{id}</div> };
          }}
        />;
        // @ts-expect-error wrong slot name
        <Tabs
          v-slot={() => {
            return { wrongSlot: () => <div /> };
          }}
        />;
      });
      it("validates required vs optional slots", () => {
        const RequiredSlots = defineComponent({
          slots: {} as SlotsType<{ header: () => any; content: () => any }>,
        });
        // @ts-expect-error missing required slot
        <RequiredSlots
          v-slot={() => {
            return { header: () => <div /> };
          }}
        />;
        <RequiredSlots
          v-slot={() => {
            return { header: () => <div />, content: () => <div /> };
          }}
        />;
      });
    });
  });

  describe("Vue lifecycle hooks (onVue:*)", () => {
    it("mounted receives instance properties", () => {
      const TestComp = defineComponent({
        setup() {
          return { value: "test", count: 42 };
        },
      });
      <TestComp
        onVue:mounted={(vnode) => {
          assertType<string>(vnode.ctx.proxy.value);
          assertType<number>(vnode.ctx.proxy.count);
        }}
      />;
    });
    it("mounted with reactive data and method", () => {
      const Counter = defineComponent({
        setup() {
          return { count: ref(0), increment: () => {} };
        },
      });
      <Counter
        onVue:mounted={(vnode) => {
          vnode.ctx.proxy.increment();
          assertType<any>(vnode.ctx.proxy.count);
        }}
      />;
    });
    it("unmounted receives cleanupId", () => {
      const TestComp = defineComponent({
        setup() {
          return { cleanupId: "cleanup-123" };
        },
      });
      <TestComp
        onVue:unmounted={(vnode) => {
          assertType<string>(vnode.ctx.proxy.cleanupId);
        }}
      />;
    });
    it("updated receives current and old vnode proxies", () => {
      const TestComp = defineComponent({
        setup() {
          return { version: ref(1) };
        },
      });
      <TestComp
        onVue:updated={(current, old) => {
          assertType<any>(current.ctx.proxy.version);
          assertType<any>(old.ctx.proxy.version);
        }}
      />;
    });
    it("before-mount receives isReady flag", () => {
      const TestComp = defineComponent({
        setup() {
          return { isReady: false };
        },
      });
      <TestComp
        onVue:before-mount={(vnode) => {
          assertType<boolean>(vnode.ctx.proxy.isReady);
        }}
      />;
    });
    it("before-unmount receives saveState method", () => {
      const TestComp = defineComponent({
        setup() {
          return { saveState: () => {} };
        },
      });
      <TestComp
        onVue:before-unmount={(vnode) => {
          assertType<() => void>(vnode.ctx.proxy.saveState);
          vnode.ctx.proxy.saveState();
        }}
      />;
    });
    it("before-update receives data ref", () => {
      const TestComp = defineComponent({
        setup() {
          return { data: ref({ id: 1 }) };
        },
      });
      <TestComp
        onVue:before-update={(current, old) => {
          assertType<{ id: number }>(current.ctx.proxy.data);
          assertType<{ id: number }>(old.ctx.proxy.data);
        }}
      />;
    });
    it("allows combining multiple hooks", () => {
      const TestComp = defineComponent({
        setup() {
          return { id: "comp-1", state: ref("initial") };
        },
      });
      <TestComp
        onVue:before-mount={(vnode) => {
          assertType<string>(vnode.ctx.proxy.id);
        }}
        onVue:mounted={(vnode) => {
          assertType<string>(vnode.ctx.proxy.id);
        }}
        onVue:before-update={(current) => {
          assertType<string>(current.ctx.proxy.id);
        }}
        onVue:updated={(current) => {
          assertType<string>(current.ctx.proxy.id);
        }}
        onVue:before-unmount={(vnode) => {
          assertType<string>(vnode.ctx.proxy.id);
        }}
        onVue:unmounted={(vnode) => {
          assertType<string>(vnode.ctx.proxy.id);
        }}
      />;
    });
    it("combines hooks with v-slot", () => {
      const TestComp = defineComponent({
        slots: {} as SlotsType<{ default: (props: { data: string }) => any }>,
        setup() {
          return { componentId: "test-123" };
        },
      });
      <TestComp
        v-slot={(c) => {
          assertType<string>(c.componentId);
          return { default: ({ data }) => <div>{data}</div> };
        }}
        onVue:mounted={(vnode) => {
          assertType<string>(vnode.ctx.proxy.componentId);
        }}
        onVue:unmounted={(vnode) => {
          assertType<string>(vnode.ctx.proxy.componentId);
        }}
      />;
    });
  });
});
