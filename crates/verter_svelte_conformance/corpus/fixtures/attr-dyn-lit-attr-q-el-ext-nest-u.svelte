<script>
	let { value } = $props();
</script>

<div data-x="{value}">x</div>

<style>
	[data-x="b"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
