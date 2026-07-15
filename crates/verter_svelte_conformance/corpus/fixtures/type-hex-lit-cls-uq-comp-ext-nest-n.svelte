<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class=a&#x20;b>x</div>
</Child>

<style>
	p {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
