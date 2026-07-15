<script lang="ts">
  // Runes props with destructuring defaults + a $bindable(<default>) fallback.
  // `size` defaults to 'md'; `disabled` defaults to false via $bindable; `label`
  // has no default (required). The defaults make `size`/`disabled` optional on
  // the surface (framework-surface `required = false`).
  interface Props { size?: 'sm' | 'md' | 'lg'; disabled?: boolean; label: string }
  let { size = 'md', disabled = $bindable(false), label }: Props = $props();
</script>
<button class={size} {disabled}>{label}</button>
