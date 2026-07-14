<script lang="ts">
  // A Svelte component whose public prop surface carries a uniquely-named prop.
  // A plain `.ts` importer must see `verterDxCaption` on the component's prop
  // surface — the §2.9 contract for the Svelte adapter.
  export let verterDxCaption: string;
</script>

<div>{verterDxCaption}</div>
