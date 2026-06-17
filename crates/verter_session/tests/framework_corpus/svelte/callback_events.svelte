<script lang="ts">
  interface Row { id: number }
  interface Props {
    label: string;
    onselect: (row: Row) => void;
    inflate: () => void;
    online: boolean;
  }
  let { label, onselect, inflate, online }: Props = $props();
</script>
<button onclick={() => onselect({ id: 1 })}>{label}{online}{inflate}</button>
