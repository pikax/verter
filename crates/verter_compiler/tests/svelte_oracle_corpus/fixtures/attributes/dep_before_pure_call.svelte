<script>
	let flag = $state(false);
</script>

<input readonly={flag || (globalThis?.check?.() ?? false)} />
