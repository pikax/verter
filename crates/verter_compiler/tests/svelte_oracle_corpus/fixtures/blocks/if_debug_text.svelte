<script>
	let show = $state(true);
	let a = $state(0);
</script>

{#if show}{@debug a}shown{/if}
