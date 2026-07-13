<script>
	let open = $state(true);
	let { flag } = $props();
</script>

{#if open}
	<div id="{flag ? 'b' : '\u0062'}">x</div>
{/if}

<style>
	#b {
		color: red;
	}
</style>
