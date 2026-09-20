<script lang="ts">
  export let title: string = "Untitled";
</script>

<header>
  <slot name="header">{title}</slot>
</header>
<slot />
