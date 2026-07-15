<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div id={flag ? 'a&b' : 'a\u0026b'}>x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="\7a z"] {
		color: red;
	}
</style>
