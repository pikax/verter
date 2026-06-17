// Curated semantic oracle - slots.
//
// The intended Vue semantics of `defineSlots<{ default(props: { item: string }): unknown }>()`:
// each slot is a render function whose first argument is the declared slot-props
// object, so the scoped-slot props (`item: string`) must reach the consumer.
//
// Each anchor's query target is the LAST identifier on its line.

interface DrawerSlots {
  default(props: { item: string }): unknown;
  header(props: { title: string }): unknown;
}

declare const slots: DrawerSlots;

const renderDefault = slots.default; // @dx-anchor slots.default
const renderHeader = slots.header; // @dx-anchor slots.header

export { renderDefault, renderHeader };
