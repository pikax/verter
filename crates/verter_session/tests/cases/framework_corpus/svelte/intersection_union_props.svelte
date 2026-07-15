<script lang="ts">
  // Intersection + union prop types. `base` (intersection) and `mode` (union)
  // both surface as named props.
  interface A { a: string }
  interface B { b: number }
  interface Props { base: A & B; mode: 'on' | 'off' }
  let { base, mode }: Props = $props();
  void base; void mode;
</script>
<div></div>
