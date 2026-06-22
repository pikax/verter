<script>
	let c = $state('x');
</script>

<button onclick={() => c += '!'} class={String(c)}>go</button>
