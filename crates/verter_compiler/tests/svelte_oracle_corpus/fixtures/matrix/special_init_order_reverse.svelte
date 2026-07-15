<script>let key = $state('');</script>
<svelte:head><title>{key || 'initial'}</title></svelte:head>
<svelte:window onkeydown={(event) => key = event.key} />
<p>{key}</p>
