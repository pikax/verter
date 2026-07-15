<script>
	let flag = $state(false);
</script>

<input readonly={(globalThis?.check?.() ?? false) || flag} />
