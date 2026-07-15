<script>let key = $state('');</script>
<svelte:window onkeydown={(event) => key = event.key} />
<svelte:head><title>{key || 'initial'}</title></svelte:head>
<p>{key}</p>
