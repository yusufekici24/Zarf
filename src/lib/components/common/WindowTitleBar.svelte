<script lang="ts">
  // İleti pencerelerinin başlık çubuğu (Outlook gibi): simge, hızlı erişim araç çubuğu
  // (Kaydet, Geri Al, Yinele, Önceki, Sonraki), pencere başlığı ve pencere düğmeleri.
  // Boş alandan sürüklenir, çift tıklayınca ekranı kaplar.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { t } from "$lib/i18n.svelte";
  import { app } from "$lib/stores/app.svelte";
  import Save from "@lucide/svelte/icons/save";
  import Undo from "@lucide/svelte/icons/undo-2";
  import Redo from "@lucide/svelte/icons/redo-2";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import Copy from "@lucide/svelte/icons/copy";
  import X from "@lucide/svelte/icons/x";

  let {
    title,
    onsave,
    onundo,
    onredo,
    onprev,
    onnext,
    onclose,
  }: {
    title: string;
    onsave?: () => void;
    onundo?: () => void;
    onredo?: () => void;
    onprev?: (() => void) | null;
    onnext?: (() => void) | null;
    onclose: () => void;
  } = $props();

  let maximized = $state(false);
  let focused = $state(true);
  onMount(() => {
    const w = getCurrentWindow();
    const refresh = () => w.isMaximized().then((m) => (maximized = m)).catch(() => {});
    refresh();
    const offs: (() => void)[] = [];
    w.onResized(refresh).then((f) => offs.push(f)).catch(() => {});
    w.onFocusChanged((e) => (focused = e.payload)).then((f) => offs.push(f)).catch(() => {});
    return () => offs.forEach((f) => f());
  });

  const qa = "flex size-7 items-center justify-center rounded hover:bg-black/10 disabled:pointer-events-none disabled:opacity-35 dark:hover:bg-white/10";
</script>

<header data-tauri-drag-region class="bg-chrome flex h-9 shrink-0 items-center gap-0.5 pl-2 select-none {focused ? '' : 'text-muted-foreground'}">
  <img src="/icon.svg" alt="" class="pointer-events-none mr-1.5 size-[18px]" />
  <!-- Hızlı Erişim Araç Çubuğu (şerit seçeneklerinden gizlenebilir) -->
  {#if app.setting("qat_hidden", "0") !== "1"}
  <button class={qa} title={t("compose.save_draft") + " (Ctrl+S)"} disabled={!onsave} onclick={() => onsave?.()}><Save class="size-4 text-[#7a3e9d]" /></button>
  <button class={qa} title={t("editor.undo") + " (Ctrl+Z)"} disabled={!onundo} onmousedown={(e) => e.preventDefault()} onclick={() => onundo?.()}><Undo class="size-4" /></button>
  <button class={qa} title={t("editor.redo") + " (Ctrl+Y)"} disabled={!onredo} onmousedown={(e) => e.preventDefault()} onclick={() => onredo?.()}><Redo class="size-4" /></button>
  <button class={qa} title={t("mail.previous")} disabled={!onprev} onclick={() => onprev?.()}><ArrowUp class="size-4" /></button>
  <button class={qa} title={t("mail.next")} disabled={!onnext} onclick={() => onnext?.()}><ArrowDown class="size-4" /></button>
  <span class="pointer-events-none flex w-5 justify-center opacity-60"><ChevronDown class="size-3.5" /></span>
  {/if}
  <div data-tauri-drag-region class="ml-3 min-w-0 flex-1 truncate text-[13px]">{title}</div>
  <div class="flex h-full items-stretch">
    <button class="flex w-11 items-center justify-center hover:bg-black/10 dark:hover:bg-white/10" title={t("window.minimize")} aria-label={t("window.minimize")} onclick={() => getCurrentWindow().minimize()}><Minus class="size-4" /></button>
    <button class="flex w-11 items-center justify-center hover:bg-black/10 dark:hover:bg-white/10" title={maximized ? t("window.restore") : t("window.maximize")} aria-label={maximized ? t("window.restore") : t("window.maximize")} onclick={() => getCurrentWindow().toggleMaximize()}>
      {#if maximized}<Copy class="size-3.5 -scale-x-100" />{:else}<Square class="size-3.5" />{/if}
    </button>
    <button class="flex w-11 items-center justify-center hover:bg-[#c42b1c] hover:text-white" title={t("common.close")} aria-label={t("common.close")} onclick={onclose}><X class="size-4" /></button>
  </div>
</header>
