<script>
	let { value } = $props();
</script>

<slot>
	<div id={value}>x</div>
</slot>

<style>
	#b {
		color: red;
	}
</style>
