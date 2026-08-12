<script>
  export let title = "Untitled";
</script>

<div class="panel">
  <header>
    <slot name="header">{title}</slot>
  </header>
  <main>
    <slot />
  </main>
</div>
