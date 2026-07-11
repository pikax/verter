<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div id={flag ? 'b' : '\u0062'}>x</div>
{/snippet}

{@render subject()}

<style>
	#b {
		color: red;
	}
</style>
