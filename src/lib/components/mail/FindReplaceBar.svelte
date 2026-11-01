<script lang="ts">
  // Yeni ileti: Bul ve Değiştir çubuğu (Ctrl+F / Ctrl+H, Gözden Geçir şeridi).
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";
  import X from "@lucide/svelte/icons/x";
  import type RichEditor from "$lib/components/common/RichEditor.svelte";
  import { t } from "$lib/i18n.svelte";

  let { editor, onclose }: { editor: RichEditor | undefined; onclose: () => void } = $props();

  let query = $state("");
  let replacement = $state("");
  let caseSensitive = $state(false);
  let input: HTMLInputElement | undefined = $state();

  onMount(() => input?.focus());

  function next() {
    if (query && !editor?.findNext(query, caseSensitive)) toast(t("editor.find_none", query));
  }

  function one() {
    if (query && !editor?.replaceCurrent(query, replacement, caseSensitive)) toast(t("editor.find_none", query));
  }

  function all() {
    if (query) toast(t("editor.replaced_n", editor?.replaceAll(query, replacement, caseSensitive) ?? 0));
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if (e.key === "Enter") {
      e.preventDefault();
      next();
    }
  }

  const btn = "hover:bg-accent h-6 rounded border px-2 text-xs disabled:opacity-40";
  const field = "h-6 w-44 rounded border bg-transparent px-1.5 text-xs outline-none focus:border-primary";
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex flex-wrap items-center gap-1.5 border-b px-4 py-1" {onkeydown}>
  <input bind:this={input} class={field} placeholder={t("editor.find")} aria-label={t("editor.find")} bind:value={query} />
  <button class={btn} disabled={!query} onclick={next}>{t("editor.find_next")}</button>
  <input class={field} placeholder={t("editor.replace_with")} aria-label={t("editor.replace_with")} bind:value={replacement} />
  <button class={btn} disabled={!query} onclick={one}>{t("editor.replace")}</button>
  <button class={btn} disabled={!query} onclick={all}>{t("editor.replace_all")}</button>
  <label class="flex items-center gap-1 text-xs"><input type="checkbox" bind:checked={caseSensitive} />{t("editor.match_case")}</label>
  <span class="flex-1"></span>
  <button class="hover:bg-accent rounded p-1" title={t("common.close")} aria-label={t("common.close")} onclick={onclose}><X class="size-3.5" /></button>
</div>
