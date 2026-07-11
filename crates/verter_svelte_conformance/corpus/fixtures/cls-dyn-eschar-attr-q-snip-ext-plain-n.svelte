<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div data-x="{flag ? 'a&b' : 'a\u0026b'}">x</div>
{/snippet}

{@render subject()}

<style>
	.\zz {
		color: red;
	}
</style>
