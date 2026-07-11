<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x=&#x62;>x</div>
</Child>

<style>
	p {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
