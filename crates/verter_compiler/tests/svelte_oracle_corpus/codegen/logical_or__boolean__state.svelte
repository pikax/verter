<script>
	let n = $state(0);
</script>

<input onclick={() => n++} disabled={n || 'a'} />
