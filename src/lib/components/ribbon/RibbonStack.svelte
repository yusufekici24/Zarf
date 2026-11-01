<script lang="ts">
  // Küçük komutların üst üste dizildiği sütun (Outlook: "Yoksay / Temizle / Önemsiz")
  import type { Snippet } from "svelte";
  let { children }: { children: Snippet } = $props();
</script>

<div class="rb-stack flex shrink-0 flex-col justify-start gap-0 pt-0.5">{@render children()}</div>
