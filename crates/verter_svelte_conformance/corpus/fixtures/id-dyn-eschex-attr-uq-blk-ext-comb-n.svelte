<script>
	let open = $state(true);
	let { flag } = $props();
</script>

{#if open}
	<div class="wrap">
	<div data-x={flag ? 'a&b' : 'a\u0026b'}>x</div>
</div>
{/if}

<style>
	.wrap #\7a z {
		color: red;
	}
</style>
