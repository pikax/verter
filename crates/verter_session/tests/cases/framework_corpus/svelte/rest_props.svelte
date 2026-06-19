<script lang="ts">
  // Rest props (`...rest`): the named props `id` / `title` surface; the rest
  // binding collects the remainder (not a named prop on the surface).
  interface Props { id: string; title?: string; [key: string]: unknown }
  let { id, title, ...rest }: Props = $props();
  void rest;
</script>
<div {id}>{title}</div>
