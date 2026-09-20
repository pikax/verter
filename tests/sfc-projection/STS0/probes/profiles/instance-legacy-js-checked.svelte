<script>
  // @ts-check
  /** @type {string} */
  export let title = "Untitled";
</script>

<header>
  <slot name="header">{title}</slot>
</header>
<slot />
