<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div id="{flag ? 'b' : '\u0062'}">x</div>
{/snippet}

{@render subject()}

<style>
	.zz {
		color: red;
	}
</style>
