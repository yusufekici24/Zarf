<script lang="ts">
  // Arayüz yazı tipi pakete gömülü (Segoe UI'ye yakın, OFL): her sistemde aynı görünüm.
  // Değişken yazı tipi değil, kalınlık başına ayrı dosya: WebKitGTK değişken yazı tipinin kalınlık eksenini
  // her çizimde uygulayamıyor (Ubuntu 24.04'te okunmamış satır üzerine gelince kalınlığını yitiriyordu).
  import "@fontsource/noto-sans/400.css";
  import "@fontsource/noto-sans/500.css";
  import "@fontsource/noto-sans/600.css";
  import "@fontsource/noto-sans/700.css";
  import "@fontsource/noto-sans/400-italic.css";
  import "@fontsource/noto-sans/700-italic.css";
  import "../app.css";
  import { invoke } from "@tauri-apps/api/core";
  import SelectPopup from "$lib/components/common/SelectPopup.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { applyUiFont, uiFontsReady } from "$lib/uifont";
  import { startPerfMonitor } from "$lib/perf";
  import { startActivityReport } from "$lib/activity";
  import { ACCENT_DEFAULT, applyAccent } from "$lib/accent";
  let { children } = $props();

  // Seçilen arayüz yazı tipi ve renk teması her pencerede (ayar başka pencerede değişince de) uygulanır
  $effect(() => applyUiFont(app.setting("ui_font", "")));
  $effect(() => applyAccent(app.setting("accent", ACCENT_DEFAULT)));

  // Gömülü yazı tipi dosyaları hemen istenir (gizli pencerede kendiliğinden inmez; bkz. uifont.ts)
  if (typeof window !== "undefined") void uiFontsReady();
  // Yavaş olaylar perf.log'a (src/lib/perf.ts)
  if (typeof window !== "undefined") startPerfMonitor();
  // Yedek pencere kullanıcı boştayken kurulur (activity.ts)
  if (typeof window !== "undefined") startActivityReport();

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

  // Metin kutularında (Kime, Konu, arama, not …) geri al / yinele. WebKitGTK Ctrl+Z'yi ve Ctrl+Y'yi kendisi
  // bağlamıyor (WebKitGTK 2.52'de denendi: tuş hiçbir şey yapmıyor, execCommand("undo"/"redo") çalışıyor).
  // Yinele: Ctrl+Y ya da Ctrl+Shift+Z. Biçimli düzenleyici kendi geçmişini kullanır (RichEditor, editor-history.ts).
  if (typeof window !== "undefined") {
    window.addEventListener(
      "keydown",
      (e) => {
        if (!(e.ctrlKey || e.metaKey) || e.altKey || e.defaultPrevented) return;
        const el = e.target as HTMLElement | null;
        const field = el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && !["checkbox", "radio", "range", "color", "button", "submit", "file"].includes(el.type));
        if (!el || !(field || el.isContentEditable) || el.closest(".rich-editor") || (el as HTMLInputElement).readOnly) return;
        const k = e.key.toLowerCase();
        const cmd = k === "z" ? (e.shiftKey ? "redo" : "undo") : k === "y" && !e.shiftKey ? "redo" : "";
        if (!cmd) return;
        e.preventDefault();
        document.execCommand(cmd);
      },
      true,
    );
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
