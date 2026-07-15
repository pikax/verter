// Curated semantic oracle - `defineProps`.
//
// The intended Vue semantics of `defineProps<{ title: string; count?: number }>()`:
// the resolved props object exposes each declared prop at its declared type, and an
// optional prop is widened to include `undefined`. The runner queries tsgo/tsserver
// on these anchors (the gold standard) and compares the type to verter-on-`.vue`.
//
// Each anchor's query target is the LAST identifier on its line.

interface DrawerProps {
  title: string;
  count?: number;
}

declare const props: DrawerProps;

const drawerTitle = props.title; // @dx-anchor props.title
const drawerCount = props.count; // @dx-anchor props.count

export { drawerCount, drawerTitle };
