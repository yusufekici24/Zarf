<script lang="ts" module>
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { app as appStore } from "$lib/stores/app.svelte";

  // Yerel taraf (links.rs): fare altındaki bağlantı ve çerçevede tıklanan mailto: bağlantısı.
  // WebKitGTK korumalı çerçevedeki dinleyicileri çalıştırmadığı için bunlar oradan gelir.
  const native = $state({ hover: "" });
  let listening = false;
  function listenNative() {
    if (listening) return;
    listening = true;
    // Yalnızca bu pencereye gönderilenler (genel "listen" diğer pencerelerinkini de alır)
    try {
      const w = getCurrentWebviewWindow();
      void w.listen<string>("app://hover-link", (e) => (native.hover = e.payload || "")).catch(() => {});
      void w.listen<string>("app://mailto", (e) => appStore.openMailto(e.payload)).catch(() => {});
    } catch {
      /* tarayıcıda */
    }
  }
</script>

<script lang="ts">
  import { fontCss, parseFont } from "$lib/fonts";
  import { readFontStack } from "$lib/uifont";
  // İleti gövdesi: HTML korumalı (betiksiz) bir iframe içinde gösterilir.
  // Uzak içerik (izleme pikselleri, uzak resimler) varsayılan olarak engellenir.
  import { textToHtml } from "$lib/format";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { people } from "$lib/stores/people.svelte";

  let {
    html = "",
    text = "",
    allowRemote = false,
    zoom = 1,
    autoHeight = false,
  }: { html?: string; text?: string; allowRemote?: boolean; zoom?: number; autoHeight?: boolean } = $props();

  let frame: HTMLIFrameElement | undefined = $state();
  let frameHeight = $state(200);
  /** Fare altındaki bağlantı (Outlook gibi sol altta gösterilir) */
  let hoverUrl = $state("");
  /** Fare ileti çerçevesinin üzerinde mi (yerel bildirim yalnızca o zaman gösterilir) */
  let overFrame = $state(false);
  listenNative();

  function pretty(href: string) {
    try {
      href = decodeURI(href);
    } catch {
      /* olduğu gibi */
    }
    return href.replace(/^mailto:/i, "");
  }
  const shownUrl = $derived(hoverUrl || (overFrame && native.hover ? pretty(native.hover) : ""));

  /**
   * E-posta düğmeleri çoğu zaman bağlantının içinde tablo olarak yazılır (<a><table>…</a>).
   * HTML kurallarına göre tablo bağlantının dışına çıkar ve düğme tıklanamaz olur (Outlook'un
   * Word motoru ise bağlantı sayar). Bağlantı, tablonun en içteki hücrelerine taşınır.
   */
  function fixBlockLinks(src: string): string {
    if (!/<a\b[^>]*>(?:(?!<\/a>)[\s\S])*?<table/i.test(src)) return src;
    return src.replace(/<a\b([^>]*)>((?:(?!<\/a>)[\s\S])*?<table[\s\S]*?)<\/a>/gi, (m, attrs: string, inner: string) => {
      const h = /\shref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(attrs);
      const url = h ? (h[1] ?? h[2] ?? h[3] ?? "") : "";
      if (!url) return m;
      const open = `<a href="${url.replace(/"/g, "&quot;")}" style="color:inherit;text-decoration:inherit;display:block">`;
      const cells = inner.replace(/(<t[dh]\b[^>]*>)((?:(?!<t[dh]\b|<\/t[dh]>)[\s\S])*)(<\/t[dh]>)/gi, (c, o: string, content: string, close: string) =>
        /<a\b/i.test(content) || !content.trim() ? c : `${o}${open}${content}</a>${close}`,
      );
      return `<div style="display:inline-block">${cells}</div>`;
    });
  }

  const plainOnly = $derived(app.setting("plain_text_only", "0") === "1");
  // Düz metin iletileri "Kırtasiye ve Yazı Tipleri"ndeki düz metin yazı tipiyle okunur
  const plainFont = $derived(fontCss(parseFont(app.setting("font_plain", ""), "font_plain")));
  // Kendi yazı tipi olmayan iletiler: Seçenekler > Posta > Okuma yazı tipi
  const readFont = $derived(`font-family:${readFontStack(app.setting("read_font", ""))};font-size:${Number(app.setting("read_font_size", "11")) || 11}pt`);

  const doc = $derived.by(() => {
    const remote = allowRemote ? " https: http:" : "";
    const csp = `default-src 'none'; img-src data: blob: cid:${remote}; style-src 'unsafe-inline'${remote}; font-src data:${remote}; media-src data:${remote};`;
    // target="_blank" korumalı çerçevede açılır pencere olarak engellenir: bağlantı çerçevenin
    // kendisinde gezinir, yerel taraf yakalayıp varsayılan tarayıcıda açar
    const body = fixBlockLinks(html && !plainOnly ? html : `<div class="plain">${textToHtml(text || "")}</div>`).replace(/(<a\b[^>]*?)\starget\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, "$1");
    const base = `<meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${csp}"><base target="_self">
<style>
html{background:#fff;color:#000}
@font-face{font-family:"Aptos";font-weight:400;font-style:normal;src:local("Noto Sans"),local("Liberation Sans")}@font-face{font-family:"Aptos";font-weight:700;font-style:normal;src:local("Noto Sans Bold"),local("Liberation Sans")}@font-face{font-family:"Aptos";font-weight:400;font-style:italic;src:local("Noto Sans Italic"),local("Liberation Sans")}@font-face{font-family:"Aptos";font-weight:700;font-style:italic;src:local("Noto Sans Bold Italic"),local("Liberation Sans")}@font-face{font-family:"Segoe UI";font-weight:400;font-style:normal;src:local("Noto Sans"),local("Liberation Sans")}@font-face{font-family:"Segoe UI";font-weight:700;font-style:normal;src:local("Noto Sans Bold"),local("Liberation Sans")}@font-face{font-family:"Segoe UI";font-weight:400;font-style:italic;src:local("Noto Sans Italic"),local("Liberation Sans")}@font-face{font-family:"Segoe UI";font-weight:700;font-style:italic;src:local("Noto Sans Bold Italic"),local("Liberation Sans")}
body{${readFont};margin:16px;word-wrap:break-word;overflow-wrap:anywhere;zoom:${zoom}}
[style*="mso-hide:all"],[style*="mso-hide: all"]{display:none!important}
img{max-width:100%;height:auto}
blockquote{margin:5pt 0 5pt 36pt}
.plain{white-space:normal;${plainFont}}
a{color:#0563c1}a:visited{color:#954f72}
pre{white-space:pre-wrap}${autoHeight ? "\nhtml{overflow-y:hidden}html,body{height:auto!important;min-height:0!important}" : ""}
</style>`;
    if (/<head[^>]*>/i.test(body)) return body.replace(/<head([^>]*)>/i, `<head$1>${base}`);
    return `<!doctype html><html><head>${base}</head><body>${body}</body></html>`;
  });

  // Belge başına bir kez dinleyici eklenir (load olayı ya da aşağıdaki yedek yol)
  const attached = new WeakSet<Document>();

  // Bazı WebKitGTK sürümlerinde srcdoc'un load olayı dinleyici bağlanmadan gelebiliyor:
  // içerik değişince belge hazır olduğunda yine bağlanır
  $effect(() => {
    void doc;
    let tries = 0;
    const tick = () => {
      const d = frame?.contentDocument;
      if (d && d.readyState === "complete" && d.body && d.documentElement.innerHTML.length > 50) attach();
      else if (tries++ < 40) timer = setTimeout(tick, 50);
    };
    let timer = setTimeout(tick, 50);
    return () => clearTimeout(timer);
  });

  function attach() {
    const d = frame?.contentDocument;
    if (!d) return;
    // Gövdenin içerik yüksekliği ölçülür (belgeninki çerçeveden küçük olamaz: çerçeve büyüdükçe
    // ölçüm de büyürdü)
    const measure = () => {
      const b = d.body;
      if (!b) return;
      const cs = d.defaultView?.getComputedStyle(b);
      const margins = parseFloat(cs?.marginTop || "0") + parseFloat(cs?.marginBottom || "0");
      const h = Math.max(80, Math.ceil(Math.max(b.scrollHeight, b.getBoundingClientRect().height) + margins) + 8);
      if (Math.abs(h - frameHeight) > 1) frameHeight = h;
    };
    if (autoHeight) measure();
    if (attached.has(d)) return;
    attached.add(d);
    // Resimler ve yazı tipleri yüklendikçe yükseklik değişir (kesilmesin). Korumalı çerçevede
    // WebKitGTK belge dinleyicilerini ve gözlemcileri çalıştırmadığı için birkaç kez yeniden ölçülür.
    if (autoHeight) for (const ms of [150, 500, 1200, 2500, 5000]) setTimeout(measure, ms);
    hoverUrl = "";
    // İleti gövdesine tıklayınca açık kişi kartı kapanır (Chrome'da; WebKit'te betiksiz çerçevede
    // dinleyici çalışmaz, orada app.css'teki kural tıklamayı çerçevenin altındaki bölmeye bırakır)
    d.addEventListener("pointerdown", () => (people.card = null));
    d.addEventListener("click", (e) => {
      const a = (e.target as HTMLElement)?.closest?.("a") as HTMLAnchorElement | null;
      if (!a) return;
      e.preventDefault();
      const href = a.getAttribute("href") ?? "";
      if (href.toLowerCase().startsWith("mailto:")) app.openMailto(href);
      else if (/^https?:|^ftp:/i.test(href)) void api.openUrl(href).catch(() => {});
    });
    d.addEventListener("mouseover", (e) => {
      const a = (e.target as HTMLElement)?.closest?.("a[href]") as HTMLAnchorElement | null;
      hoverUrl = a ? pretty(a.getAttribute("href") ?? "") : "";
    });
    d.addEventListener("mouseleave", () => (hoverUrl = ""));
    d.documentElement.addEventListener("mouseleave", () => (hoverUrl = ""));
  }

  export function print() {
    frame?.contentWindow?.print();
  }
</script>

<div class="relative {autoHeight ? '' : 'h-full'}">
<!-- svelte-ignore a11y_no_static_element_interactions -->
<iframe
  bind:this={frame}
  title="message"
  class="w-full border-0 bg-white {autoHeight ? '' : 'h-full'}"
  style={autoHeight ? `height:${frameHeight}px` : ""}
  sandbox="allow-same-origin allow-modals"
  srcdoc={doc}
  onload={attach}
  onmouseenter={() => (overFrame = true)}
  onmouseleave={() => { overFrame = false; native.hover = ""; }}
></iframe>
{#if shownUrl}
  <div class="bg-popover text-popover-foreground pointer-events-none absolute bottom-1 left-1 z-10 max-w-[80%] truncate rounded border px-2 py-0.5 text-xs shadow-md">{shownUrl}</div>
{/if}
</div>
