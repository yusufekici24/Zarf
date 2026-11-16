<script lang="ts">
  // Bölünmüş renk düğmesi (Outlook yazı rengi / vurgu rengi): düğme son rengi hemen uygular,
  // yanındaki ok paleti açar. Yerel renk kutusu (<input type="color">) kullanılmaz: o yalnızca
  // değer değişince olay gönderdiği için aynı renk ikinci kez seçilince hiçbir şey olmuyordu.
  // Düğmeler odağı almaz (mousedown engellenir): düzenleyicideki seçim palet açıkken de görünür kalır.
  import type { Component } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Ban from "@lucide/svelte/icons/ban";
  import * as Popover from "$lib/components/ui/popover";
  import { HIGHLIGHT_COLORS, STANDARD_COLORS, THEME_COLORS, normalizeHex, type ColorKind } from "$lib/editor";
  import { editorColors } from "$lib/editor-colors.svelte";
  import { t } from "$lib/i18n.svelte";

  let {
    icon: Icon,
    label,
    kind,
    onpick,
    disabled = false,
  }: {
    icon: Component<{ class?: string }>;
    label: string;
    kind: ColorKind;
    /** Boş renk: otomatik (yazı) ya da renk yok (vurgu, gölgelendirme) */
    onpick: (color: string) => void;
    disabled?: boolean;
  } = $props();

  let open = $state(false);
  let hex = $state("");
  const color = $derived(editorColors[kind]);
  const custom = $derived(normalizeHex(hex));
  const hold = (e: Event) => e.preventDefault();

  function choose(c: string) {
    if (c) editorColors.use(kind, c);
    open = false;
    onpick(c);
  }
</script>

{#snippet swatch(c: string, cls: string)}
  <button type="button" class="{cls} hover:ring-foreground/70 rounded-[2px] border border-black/15 hover:ring-2 dark:border-white/25" style="background:{c}" title={c} aria-label={c} onmousedown={hold} onclick={() => choose(c)}></button>
{/snippet}

<div class="flex items-stretch" data-color={kind}>
  <button type="button" class="ed-btn flex-col !rounded-r-none !px-1 !pt-[3px] !pb-[2px]" title={label} aria-label={label} {disabled} onmousedown={hold} onclick={() => onpick(color)}>
    <Icon class="size-[15px]" />
    <span class="mt-px h-1 w-4 rounded-[1px] outline outline-1 outline-black/20 dark:outline-white/30" style="background:{color}"></span>
  </button>
  <Popover.Root bind:open>
    <Popover.Trigger {disabled}>
      {#snippet child({ props })}
        <button {...props} type="button" class="ed-btn !rounded-l-none !px-0" data-active={open} title={t("editor.color_more", label)} aria-label={t("editor.color_more", label)} onmousedown={hold}><ChevronDown class="size-3 opacity-70" /></button>
      {/snippet}
    </Popover.Trigger>
    <Popover.Content align="start" class="w-auto gap-2 rounded-[4px] p-2 text-[12px]" onOpenAutoFocus={hold} onCloseAutoFocus={hold}>
      <button type="button" class="hover:bg-accent flex items-center gap-2 rounded px-1.5 py-1 text-left" onmousedown={hold} onclick={() => choose("")}>
        <Ban class="text-muted-foreground size-4" />{kind === "fore" ? t("editor.color_auto") : t("editor.color_none")}
      </button>
      {#if kind === "back"}
        <div class="grid grid-cols-5 gap-1" role="group" aria-label={t("editor.highlight")}>
          {#each HIGHLIGHT_COLORS as c (c)}{@render swatch(c, "h-6 w-9")}{/each}
        </div>
      {:else}
        <div>
          <div class="text-muted-foreground mb-1 font-semibold">{t("editor.colors_theme")}</div>
          <div class="grid grid-flow-col grid-rows-6 gap-x-1 gap-y-px">
            {#each THEME_COLORS as col, i (i)}
              {#each col as c, j (j)}{@render swatch(c, `size-[18px] ${j === 0 ? "mb-1" : ""}`)}{/each}
            {/each}
          </div>
        </div>
        <div>
          <div class="text-muted-foreground mb-1 font-semibold">{t("editor.colors_standard")}</div>
          <div class="flex gap-1">{#each STANDARD_COLORS as c (c)}{@render swatch(c, "size-[18px]")}{/each}</div>
        </div>
      {/if}
      {#if editorColors.recent.length}
        <div>
          <div class="text-muted-foreground mb-1 font-semibold">{t("editor.colors_recent")}</div>
          <div class="flex gap-1">{#each editorColors.recent as c (c)}{@render swatch(c, "size-[18px]")}{/each}</div>
        </div>
      {/if}
      <!-- Özel renk: kod yazılır ya da kutudan seçilir; "Uygula" değer değişmese de çalışır -->
      <form class="flex items-center gap-1.5 border-t pt-2" onsubmit={(e) => { e.preventDefault(); if (custom) choose(custom); }}>
        <input type="color" class="size-6 shrink-0 cursor-pointer rounded border p-0" value={custom || color} title={t("editor.colors_custom")} aria-label={t("editor.colors_custom")} oninput={(e) => (hex = (e.target as HTMLInputElement).value)} />
        <input class="focus:border-primary h-6 w-20 rounded border bg-transparent px-1.5 font-mono text-[12px] outline-none" placeholder="#RRGGBB" maxlength="7" spellcheck="false" aria-label={t("editor.colors_code")} bind:value={hex} />
        <button type="submit" class="hover:bg-accent h-6 rounded border px-2 disabled:opacity-40" disabled={!custom}>{t("common.apply")}</button>
      </form>
    </Popover.Content>
  </Popover.Root>
</div>
