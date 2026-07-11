<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div class="wrap">
	<div class={value}>x</div>
</div>
{/if}

<style>
	.wrap .b {
		color: red;
	}
</style>
