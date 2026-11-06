<script lang="ts">
  // Renk teması seçici: renk düğmeleri (üst çubuktaki palet ve Seçenekler > Genel > Renk)
  import { ACCENTS } from "$lib/accent";
  import { t } from "$lib/i18n.svelte";
  import Check from "@lucide/svelte/icons/check";

  let { value, onpick }: { value: string; onpick: (key: string) => void } = $props();
</script>

<div class="flex flex-wrap gap-1.5" role="radiogroup" aria-label={t("accent.title")}>
  {#each ACCENTS as a (a.key)}
    {@const on = value === a.key}
    <button
      type="button"
      role="radio"
      aria-checked={on}
      aria-label={t(a.label)}
      title={t(a.label)}
      class="ring-offset-background flex size-7 items-center justify-center rounded-full ring-offset-2 transition-shadow {on ? 'ring-foreground/60 ring-2' : 'hover:ring-foreground/25 hover:ring-2'}"
      style="background:{a.color}"
      onclick={() => onpick(a.key)}
    >
      {#if on}<Check class="size-4 text-white" strokeWidth={3} />{/if}
    </button>
  {/each}
</div>
