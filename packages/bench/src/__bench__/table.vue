<template>
  <div class="h-table-container">
    <div v-if="currentBreakpoint" class="flex justify-between md:px-3 md:py-4">
      <slot name="filter">
        <div />
      </slot>
      <div class="flex flex-initial">
        <h-input v-model="filterKeywork" :placeholder="i18n.search" />
      </div>
    </div>
    <div class="block overflow-y-auto">
      <table class="w-full table-auto overflow-y-auto">
        <thead>
          <tr>
            <th
              v-for="c in columns"
              :key="c.path"
              class="sticky top-0 z-50 bg-gray-200"
              :class="{ 'hidden md:table-cell': c.mobile === false }"
            >
              <div class="flex justify-center">
                <slot :name="`head:${c.path}`" v-bind="c">
                  {{ c.title }}
                </slot>
                <h-icon v-if="c.sort" icon="ic:outline-arrow-downward" />
              </div>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="(item, i) in pagination.result"
            :key="i"
            class="hover:bg-gray-300"
          >
            <td
              v-for="(c, columnIndex) in columns"
              :key="c.path"
              class="p-0"
              :class="[
                !navigateTo && cellClass,
                c.class,
                { 'hidden md:table-cell': c.mobile === false },
              ]"
            >
              <template v-if="navigateTo">
                <router-link
                  :to="navigateTo(item, i)"
                  class="block"
                  :class="cellClass"
                >
                  <slot
                    :name="`item:${c.name || c.path}`"
                    v-bind="{
                      item,
                      index: i,
                      navigateTo,
                      value: accessors[columnIndex](item),
                    }"
                  >
                    {{ accessors[columnIndex](item) }}
                  </slot>
                </router-link>
              </template>
              <template v-else>
                <slot
                  :name="`item:${c.name || c.path}`"
                  v-bind="{
                    item,
                    index: i,
                    navigateTo,
                    value: accessors[columnIndex](item),
                  }"
                >
                  {{ accessors[columnIndex](item) }}
                </slot>
              </template>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <div class="flex items-center gap-1 md:gap-2">
      <h-button icon="ic:round-chevron-left" @click="pagination.prev" />
      <h-button icon="ic:round-chevron-right" @click="pagination.next" />

      <span>{{ i18n.pageNumber }} </span>
      <h-select
        v-if="currentBreakpoint"
        v-model="pagination.pageSize"
        :options="[
          { label: '10', value: 10 },
          { label: '20', value: 20 },
          { label: '50', value: 50 },
          { label: '100', value: 100 },
          { label: '200', value: 200 },
        ]"
      />
      <span class="hidden md:block">{{ i18n.pageResults }}</span>

      <slot v-if="!currentBreakpoint" name="filter"> </slot>
      <div v-if="!currentBreakpoint" class="flex-auto"></div>
      <div v-if="!currentBreakpoint" class="flex flex-initial">
        <h-input v-model="filterKeywork" :placeholder="i18n.search" />
      </div>
    </div>
  </div>
</template>
<script lang="ts">
import {
  type PropType,
  computed,
  defineComponent,
  reactive,
  ref,
  watch,
} from "vue";
import HIcon from "./icon.vue";
import HSelect from "./select.vue";
import HButton from "./button.vue";
import HInput from "./input.vue";

import {
  type RefTyped,
  NO_OP,
  useArrayPagination,
  useBreakpointTailwindCSS,
  useI18n,
  useValueSync,
} from "vue-composable";

import type { RouteLocationRaw } from "vue-router";
import { useQuery } from "../../utils";

export interface ColumnDef<TItem = any> {
  path?: string;
  name?: string;
  title: RefTyped<string>;

  class?: string;

  mobile?: boolean;

  filter?: (
    keyword: string,
    value: any | undefined,
    item?: TItem,
    visible?: boolean
  ) => boolean;

  sort?: (
    value1: unknown,
    value2: unknown,
    item1: TItem,
    item2: TItem,
    descending: boolean
  ) => number;

  visible?: Boolean;

  order?: Number;
}

const _component = defineComponent({
  name: "HTable",
  components: {
    HIcon,
    HSelect,
    HButton,
    HInput,
  },
  props: {
    items: {
      type: Array,
      required: true,
    },
    columns: Array as () => ColumnDef[],

    cellClass: String,

    navigateTo: Function as PropType<
      (item: any, index?: number) => RouteLocationRaw
    >,

    pageSize: {
      type: Number,
      default: 10,
    },

    syncRouter: Boolean,
  },
  setup(props) {
    const columns = computed(() => {
      // console.log("cols", props.columns, props.items);
      if (props.columns) {
        return props.columns;
      }
      if (!props.items || props.items.length === 0) {
        return [] as ColumnDef[];
      }

      const keys = Object.keys(props.items[0] as any);
      return keys.map(
        (x) =>
          ({
            path: x,
            title: x,
          } as ColumnDef)
      );
    });

    const accessors = computed(() => {
      return columns.value.map((x) => {
        if (!x.path) {
          return NO_OP;
        }
        if (x.path.indexOf(".") >= 0) {
          return new Function("item", `return item.${x.path}`);
        }
        // @ts-ignore
        return (item: any) => item[x.path];
      });
    });

    const filterKeywork = ref("");

    const filteredItems = computed(() => {
      const search = filterKeywork.value.toLowerCase();
      let items = props.items;

      if (filterKeywork.value) {
        const found = [];
        const noFound = [...items];

        if (columns.value) {
          for (let colIndex = 0; colIndex < columns.value.length; colIndex++) {
            const col = columns.value[colIndex];
            const filter = col.filter;
            if (!filter) continue;
            const accessor = accessors.value[colIndex];

            items = items.filter((x: any) =>
              filter(search, accessor(x), x, true)
            );

            const foundIndex = [];
            for (let i = 0; i < noFound.length; i++) {
              const item = noFound[i];
              const f = filter(search, accessor(item), item, true);
              if (f) {
                found.push(item);
                foundIndex.push(i);
              }
            }

            foundIndex.forEach((i) => noFound.splice(i, 1));
          }
        }
        items = found;
      }
      return items;
    });

    const sortedItems = computed(() => {
      let items = [...filteredItems.value];
      for (let colIndex = 0; colIndex < columns.value.length; colIndex++) {
        const col = columns.value[colIndex];
        const sort = col.sort;
        if (!sort) continue;
        const accessor = accessors.value[colIndex];

        items.sort((a, b) => sort(accessor(a), accessor(b), a, b, false));
      }
      return items;
    });

    const pagination = useArrayPagination(sortedItems, {
      pageSize: props.pageSize,
    });
    // reset items
    watch(sortedItems, () => (pagination.offset.value = 0));

    if (props.syncRouter) {
      const page = useQuery(
        "page",
        pagination.currentPage.value < 2
          ? "1"
          : `${pagination.currentPage.value}`
      );
      const size = useQuery("size", `${pagination.pageSize.value}`);

      useValueSync(
        computed<number>({
          get() {
            return +page.value!;
          },
          set(v) {
            page.value = v < 2 ? "1" : v.toString();
          },
        }),
        pagination.currentPage
      );
      useValueSync(
        computed<number>({
          get() {
            return +size.value!;
          },
          set(v) {
            size.value = v.toString();
          },
        }),
        pagination.pageSize
      );
    }
    const { $t } = useI18n();

    const { current: currentBreakpoint } = useBreakpointTailwindCSS();

    const i18n = reactive({
      pageNumber: $t(
        `app.table.pagination.${
          currentBreakpoint.value ? "page" : "small-page"
        }`,
        [
          computed(() =>
            pagination.total.value === 0 ? 0 : pagination.offset.value
          ),
          computed(() => {
            const last = pagination.offset.value + pagination.pageSize.value;
            return Math.min(last, pagination.total.value);
          }),
          pagination.total,
        ]
      ),
      pageResults: $t("app.table.pagination.pageResults"),

      search: $t("app.table.header.search"),
    });

    return {
      columns,

      accessors,
      filterKeywork,

      filteredItems,

      pagination: reactive(pagination),

      i18n,

      currentBreakpoint,
    };
  },
});

type ItemSlotKey<Type extends string, R> = {
  [K in Type as `${string & K}:${string}`]: R;
};

// type Item<Type extends string> = {
//   [x: string as `${Type}:${string}`]: { a: 1 };
// };

/*

        item,
        index: i,
        navigateTo,
        value: accessors[columnIndex](item),
                    */

type Slots = ItemSlotKey<"head", ColumnDef> &
  ItemSlotKey<
    "item",
    {
      item: any;
      navigateTo: Function;
      value: any;
    }
  >;
// {
//   [K in ItemSlotKey<"head">]: ColumnDef;
// } & {
//   // [K in ItemSlotKey<"head">]: ColumnDef;
// };

// declare const RRR: Record<keyof ItemSlotKey<"head">, ColumnDef>;

// RRR["head:"] = {};
// RRR["head:aaaa"] = { title: "12312" };
// &
//   {
//     [K in ItemSlotKey<"item">]: {
//       item: any;
//       navigateTo: Function;
//       value: any;
//     };
//   } & {

//   };

// declare const DDD: Slots;

// DDD["head:"] = { title };
// DDD["head:sss"] = {};
// DDD["head:asda"] = {};
// DDD["head:12314"] = {};
// DDD["head:1234"] = { title: "1" };

// // DDD['head:1221'].
// DDD["head:aa"].a.toString()

export default _component as typeof _component & {
  __VLS_slots: Slots;
};
// export default _component;
</script>
<style lang="scss">
.h-table-container {
  display: grid;

  grid-template-rows: 1fr 3em;

  @media (min-width: 640px) {
    grid-template-rows: 5em 1fr auto;
  }
  // grid-template-rows: 30px 1fr 30px;
}
</style>
