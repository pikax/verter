<script>
	import Alpha from './Alpha.svelte';
	import Beta from './Beta.svelte';
	let { p } = $props();
</script>

<Alpha /><Beta />{p}
