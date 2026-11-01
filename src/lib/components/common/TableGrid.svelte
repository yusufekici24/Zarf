<script lang="ts">
  // Outlook "Tablo" ızgarası: fareyle satır × sütun seçilir, tıklayınca eklenir.
  // Altta "Tablo Ekle..." elle boyut girmek için.
  import { t } from "$lib/i18n.svelte";

  let { onpick, oncustom }: { onpick: (rows: number, cols: number) => void; oncustom?: () => void } = $props();
  const COLS = 10;
  const ROWS = 8;
  let hr = $state(0);
  let hc = $state(0);
</script>

<div class="p-2 select-none" role="grid" tabindex="-1" onmouseleave={() => { hr = 0; hc = 0; }}>
  <div class="mb-1.5 text-[12px] font-semibold">{hr && hc ? t("editor.table_n", hc, hr) : t("editor.insert_table")}</div>
  <div class="grid gap-[3px]" style="grid-template-columns:repeat({COLS},14px)">
    {#each Array(ROWS) as _, r (r)}
      {#each Array(COLS) as __, c (c)}
        <button
          type="button"
          aria-label="{c + 1}×{r + 1}"
          class="size-[14px] border {r < hr && c < hc ? 'border-primary bg-primary/25' : 'border-[#bfbfbf] bg-background'}"
          onmouseenter={() => { hr = r + 1; hc = c + 1; }}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => onpick(r + 1, c + 1)}
        ></button>
      {/each}
    {/each}
  </div>
  {#if oncustom}
    <button type="button" class="hover:bg-accent mt-2 w-full rounded px-2 py-1 text-left text-[12px]" onmousedown={(e) => e.preventDefault()} onclick={oncustom}>{t("editor.insert_table")}...</button>
  {/if}
</div>
