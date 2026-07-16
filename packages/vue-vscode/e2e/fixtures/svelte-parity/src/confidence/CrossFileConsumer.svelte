<script lang="ts">
  import type { ConfidenceItem } from "./CrossFileTypes";
  import StrictChild from "../strict/StrictChild.svelte";

  const good: ConfidenceItem = { id: "a", label: "A", count: 1 };
</script>

<!-- good -->
<StrictChild label={good.label} />
<!-- Live error: count is number, pass number where string label expected -->
<StrictChild label={good.count} />
