<script lang="ts" generics="T">
  // A generic component: the props type is parameterised by `T`. The named
  // props `items` / `selected` surface; their types stay shallow.
  interface Props<T> { items: T[]; selected?: T }
  let { items, selected }: Props<T> = $props();
  void items; void selected;
</script>
<ul></ul>
