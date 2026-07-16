<script>
  // @ts-check
  /** @type {{ label: string }} */
  let jsRenameValue = { label: "js-rename" };
  /** @param {{ label: string }} value */
  function renderJsRename(value) {
    return value.label;
  }
  let jsRenameOut = renderJsRename(jsRenameValue);
</script>

<p>{jsRenameOut} {jsRenameValue.label}</p>
