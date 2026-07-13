<script>
	let { value } = $props();
</script>

<slot>
	<div class={value}>x</div>
</slot>

<style>
	.a\26 b\&c {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
