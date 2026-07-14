// The imported prop type the Vue macro traverses across the import graph.
// `label` is `string`; the deliberate `const wrong: number = props.label`
// mis-assignment in Comp.vue depends on that to produce TS2322.
export interface LabelProps {
  label: string;
  count: number;
}
