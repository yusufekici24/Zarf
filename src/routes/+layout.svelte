<script lang="ts">
  // Arayüz yazı tipi pakete gömülü (Segoe UI'ye yakın, OFL): her sistemde aynı görünüm
  import "@fontsource-variable/noto-sans/wght.css";
  import "../app.css";
  import { invoke } from "@tauri-apps/api/core";
  import SelectPopup from "$lib/components/common/SelectPopup.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { applyUiFont } from "$lib/uifont";
  let { children } = $props();

  // Seçilen arayüz yazı tipi her pencerede (ayar başka pencerede değişince de) uygulanır
  $effect(() => applyUiFont(app.setting("ui_font", "")));

  // Yakalanmayan ön yüz hatalarını uygulama günlüğüne aktar
  const log = (level: string, message: unknown) => {
    try {
      void invoke("frontend_log", { level, message: String(message instanceof Error ? `${message.message}\n${message.stack ?? ""}` : message) });
    } catch {
      /* yoksay */
    }
  };
  if (typeof window !== "undefined") {
    window.addEventListener("error", (e) => log("error", e.error ?? e.message));
    window.addEventListener("unhandledrejection", (e) => log("error", e.reason));
  }

  // Açık menü/panel/açılır liste/kişi kartı varken <html data-layer-open>: ileti gövdesi
  // çerçevesi tıklama almaz (app.css). Yalnızca öğe eklenip çıkarılınca bir kez denetlenir.
  if (typeof window !== "undefined") {
    const SEL = '[data-slot="popover-content"], [data-slot="dropdown-menu-content"], [data-slot="dropdown-menu-sub-content"], [data-slot="context-menu-content"], [data-slot="select-content"], [data-contact-card], [data-select-popup]';
    let queued = false;
    const check = () => {
      queued = false;
      document.documentElement.toggleAttribute("data-layer-open", !!document.querySelector(SEL));
    };
    const start = () =>
      new MutationObserver(() => {
        if (!queued) {
          queued = true;
          queueMicrotask(check);
        }
      }).observe(document.body, { childList: true, subtree: true });
    if (document.body) start();
    else window.addEventListener("DOMContentLoaded", start, { once: true });
  }
</script>

{@render children()}
<!-- Tüm açılır seçim kutuları uygulamanın menü görünümüyle açılır -->
<SelectPopup />
