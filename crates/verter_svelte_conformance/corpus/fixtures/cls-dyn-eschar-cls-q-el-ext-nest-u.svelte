<script>
	let { value } = $props();
</script>

<div class="{value}">x</div>

<style>
	.a\&b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
