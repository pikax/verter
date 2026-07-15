<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x=&#98;&#x62;>x</div>
</Child>

<style>
	[data-x="bb"] {
		color: red;
	}
</style>
