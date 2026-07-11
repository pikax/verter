<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div class="wrap">
	<div id="{flag ? 'b' : '\u0062'}">x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap p {
		color: red;
	}
</style>
