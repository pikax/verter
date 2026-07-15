<script>
	let disabled = $state(false);
	let readonly = $state(false);
</script>

<button onclick={() => disabled = !disabled} disabled={disabled}></button>
<input onclick={() => readonly = !readonly} readonly={readonly} />
