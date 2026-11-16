<script lang="ts">
  // Biçimlendirmeli e-posta düzenleyicisi (contenteditable + biçim araç çubuğu).
  import { t } from "$lib/i18n.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import LinkIcon from "@lucide/svelte/icons/link";
  import ImageIcon from "@lucide/svelte/icons/image";
  import TableIcon from "@lucide/svelte/icons/table";
  import Undo from "@lucide/svelte/icons/undo-2";
  import Redo from "@lucide/svelte/icons/redo-2";
  import Minus from "@lucide/svelte/icons/minus";
  import Unlink from "@lucide/svelte/icons/unlink";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import EditorTools from "$lib/components/editor/EditorTools.svelte";
  import { blockStyleCss, caretState, changeCase as convertCase, normalizeUrl, portal, textStats, type CaretState, type CaseMode, type ColorKind, type EditorApi, type TableOp } from "$lib/editor";
  import { applyTableStyle, tableOp as runTableOp } from "$lib/editor-table";
  import { FONT_SIZES } from "$lib/fonts";
  import { escapeHtml } from "$lib/format";
  import { i18n } from "$lib/i18n.svelte";

  let {
    html = $bindable(""),
    plain = false,
    toolbar = true,
    onchange,
    fontStyle = "",
    caret = $bindable(caretState()),
    onfiles,
    zoom = 1,
    onactivity,
  }: {
    html?: string;
    plain?: boolean;
    toolbar?: boolean;
    onchange?: () => void;
    /** Varsayılan yazı tipi (Kırtasiye ve Yazı Tipleri) */
    fontStyle?: string;
    /** İmlecin bulunduğu yerdeki biçim (şeritteki kutular ve düğmeler bunu gösterir, Outlook gibi) */
    caret?: CaretState;
    /** Yapıştırılan resim olmayan dosyalar (yol listesi): ek olarak eklenir */
    onfiles?: (paths: string[]) => void;
    /** Yazı alanının yakınlaştırması (durum çubuğundaki kaydırıcı); gönderilen iletiyi etkilemez */
    zoom?: number;
    /** İçerik ya da seçim değişti (durum çubuğu sözcük sayısını yeniler) */
    onactivity?: () => void;
  } = $props();

  let editor: HTMLDivElement | undefined = $state();
  let lastSet = "";
  let pending = false;
  let syncTimer: ReturnType<typeof setTimeout> | undefined;

  // Dışarıdan gelen içeriği yalnızca değiştiğinde yaz (imleci korumak için).
  // Yazarken içerik her tuşta seri hale getirilmez (büyük alıntılı yanıtlarda takılma
  // yapıyordu); `html` kısa bir beklemeden sonra güncellenir, gönderirken getHtml() kullanılır.
  $effect(() => {
    const h = html;
    if (editor && h !== lastSet) {
      editor.innerHTML = h;
      lastSet = h;
      onactivity?.();
    }
  });

  function flush() {
    clearTimeout(syncTimer);
    if (!editor || !pending) return;
    pending = false;
    dirtyNotified = false;
    lastSet = editor.innerHTML;
    html = lastSet;
  }

  let dirtyNotified = false;
  function sync() {
    if (!editor) return;
    pending = true;
    fixFontSize();
    onactivity?.();
    // Yazarken hiçbir şey kopyalanmaz (uzun alıntılı yanıtta takılma yapıyordu): içerik
    // odak çıkınca ya da istendiğinde (gönder/kaydet: getHtml) alınır
    if (!dirtyNotified) {
      dirtyNotified = true;
      onchange?.();
    }
  }

  /** Düzenleyicinin güncel HTML'i */
  export function getHtml(): string {
    // Kullanıcı yazdıysa düzenleyicideki içerik, yazmadıysa (henüz düzenleyiciye
    // aktarılmamış olabilecek) `html` esas alınır — yanıtta alıntı kaybolmaz
    if (pending) flush();
    return html;
  }

  // Enter yeni <div> açar (Outlook gibi tek satır); <p> paragraf boşluğu bırakmaz
  $effect(() => {
    if (!editor) return;
    try {
      document.execCommand("defaultParagraphSeparator", false, "div");
    } catch {
      /* desteklenmiyor */
    }
  });

  // İmlecin düzenleyicideki son yeri: araç çubuğu/iletişim kutusu odağı alınca kaybolmasın
  // (WebKit'te odak geri verilince seçim boş kalıyor ve komut hiçbir şey yapmıyordu)
  let savedRange: Range | null = null;
  function saveRange() {
    const sel = window.getSelection();
    if (editor && sel && sel.rangeCount && editor.contains(sel.anchorNode)) savedRange = sel.getRangeAt(0).cloneRange();
  }
  $effect(() => {
    const h = () => {
      saveRange();
      const sel = window.getSelection();
      if (!editor || !sel?.rangeCount || !editor.contains(sel.anchorNode)) return;
      // Seçim kalkınca seçim araç çubuğu da kalkar
      if (sel.isCollapsed) bubble = null;
      onactivity?.();
    };
    document.addEventListener("selectionchange", h);
    return () => document.removeEventListener("selectionchange", h);
  });
  function restoreRange() {
    if (!editor) return;
    // Odak düzenleyicideyse seçim yerindedir; değilse odak verilince tarayıcının koyduğu imlece
    // güvenilmez, son kaydedilen seçim geri yüklenir
    const had = document.activeElement === editor;
    editor.focus();
    const sel = window.getSelection();
    if (!sel) return;
    if (had && sel.rangeCount && editor.contains(sel.anchorNode)) return;
    sel.removeAllRanges();
    if (savedRange && editor.contains(savedRange.startContainer)) sel.addRange(savedRange);
    else {
      const r = document.createRange();
      r.setStart(editor, 0);
      r.collapse(true);
      sel.addRange(r);
    }
  }

  export function exec(cmd: string, value?: string) {
    restoreRange();
    document.execCommand(cmd, false, value);
    sync();
    readCaret();
  }

  /** Seçili metnin puntosu (Outlook gibi pt cinsinden: 8, 9, 10, 10.5, 11, 12...) */
  export function setFontSize(pt: number) {
    if (!editor) return;
    restoreRange();
    // (komutun gönderdiği "input" olayı fixFontSize'ı çağırır: punto ondan önce yazılmalı)
    pendingSize = pt;
    document.execCommand("styleWithCSS", false, "false");
    document.execCommand("fontSize", false, "7");
    sync();
    readCaret();
  }

  // Tarayıcının verdiği <font size="7"> yerinde punto stiline çevrilir (öğe değiştirilmez ki seçim
  // bozulmasın). Seçim boşken komut yalnızca "yazılacak metnin" boyutunu ayarlar; öğe ilk tuşta
  // oluşur, bu yüzden her girdide yeniden bakılır (yoksa yazılan metin dev boyutta kalıyordu).
  let pendingSize = 0;
  function fixFontSize() {
    if (!pendingSize || !editor) return;
    editor.querySelectorAll<HTMLElement>('font[size="7"]').forEach((f) => {
      f.removeAttribute("size");
      f.style.fontSize = `${pendingSize}pt`;
    });
  }

  /** Yazı tipini bir kademe büyütür/küçültür (Ctrl+Shift+> / Ctrl+<) */
  export function stepFontSize(dir: number) {
    const cur = caret.size || 11;
    const next = dir > 0 ? FONT_SIZES.find((s) => s > cur) : FONT_SIZES.findLast((s) => s < cur);
    if (next) setFontSize(next);
  }

  /** Yazı rengi, vurgu rengi ya da (tabloda) hücre gölgesi. Boş renk: otomatik / renk yok */
  export function setColor(kind: ColorKind, color: string) {
    if (!editor) return;
    if (kind === "shade") return tableOp("shade", color);
    restoreRange();
    document.execCommand("styleWithCSS", false, "true");
    if (kind === "fore") document.execCommand("foreColor", false, color || getComputedStyle(editor).color);
    else document.execCommand("hiliteColor", false, color || "transparent");
    document.execCommand("styleWithCSS", false, "false");
    sync();
    readCaret();
  }

  /** Paragraf stili (Başlık 1, Alıntı, Kod …); aynı stil yeniden seçilirse normale döner */
  export function setBlock(tag: string) {
    const to = caret.block === tag ? "div" : tag;
    // Alıntı bloğu biçim değiştirilerek kalkmaz: önce girinti azaltılır
    if (caret.block === "blockquote") exec("outdent");
    if (to !== "div" || caret.block !== "blockquote") exec("formatBlock", `<${to}>`);
  }

  const BLOCKS = "div,p,li,h1,h2,h3,h4,h5,h6,blockquote,pre,td,th";
  /** Seçimin değdiği en içteki bloklar (paragraflar, liste öğeleri, hücreler) */
  function selectedBlocks(): HTMLElement[] {
    const sel = window.getSelection();
    if (!editor || !sel?.rangeCount) return [];
    const r = sel.getRangeAt(0);
    return Array.from(editor.querySelectorAll<HTMLElement>(BLOCKS)).filter((b) => r.intersectsNode(b) && !b.querySelector(BLOCKS));
  }

  /** Satır aralığı (1 = tek, 1.5, 2 …): seçimdeki paragraflara yazılır */
  export function setLineHeight(value: string) {
    restoreRange();
    let blocks = selectedBlocks();
    if (!blocks.length) {
      // Metin doğrudan düzenleyicinin içindeyse önce bir paragrafa alınır
      document.execCommand("formatBlock", false, "<div>");
      blocks = selectedBlocks();
    }
    for (const b of blocks) b.style.lineHeight = value;
    sync();
  }

  /** Seçili metnin harflerini çevirir (BÜYÜK, küçük, Her Sözcük, Tümce düzeni); biçim korunur */
  export function changeCase(mode: CaseMode) {
    restoreRange();
    const sel = window.getSelection();
    if (!editor || !sel?.rangeCount || sel.isCollapsed) return;
    const r = sel.getRangeAt(0);
    const parts: { node: Text; from: number; to: number }[] = [];
    const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode() as Text | null; n; n = walker.nextNode() as Text | null) {
      if (!r.intersectsNode(n)) continue;
      const from = n === r.startContainer ? r.startOffset : 0;
      const to = n === r.endContainer ? r.endOffset : n.length;
      if (to > from) parts.push({ node: n, from, to });
    }
    if (!parts.length) return;
    // Tümce ve sözcük başları seçimin bütününe göre bulunur (biçim bir sözcüğü bölmüş olabilir)
    const text = parts.map((p) => p.node.data.slice(p.from, p.to)).join("");
    const out = convertCase(text, mode, i18n.locale);
    let at = 0;
    for (const p of parts) {
      const len = p.to - p.from;
      // Uzunluğu değiştiren dönüşümde (ß → SS) parçalar ayrı ayrı çevrilir
      p.node.replaceData(p.from, len, out.length === text.length ? out.slice(at, at + len) : convertCase(p.node.data.slice(p.from, p.to), mode, i18n.locale));
      at += len;
    }
    const first = parts[0];
    const last = parts[parts.length - 1];
    const range = document.createRange();
    range.setStart(first.node, first.from);
    range.setEnd(last.node, Math.min(last.to, last.node.length));
    sel.removeAllRanges();
    sel.addRange(range);
    sync();
  }

  // ---------------------------------------------------------------- biçim boyacısı
  type Paint = { bold: boolean; italic: boolean; underline: boolean; strike: boolean; fore: string; back: string; family: string; size: number };
  let paint: Paint | null = null;
  function stopPainter() {
    paint = null;
    if (caret.painter) caret = { ...caret, painter: 0 };
  }
  /** Biçim Boyacısı: imleçteki biçimi alır, sonra fareyle seçilen metne uygular. `lock` (çift
      tıklama): Esc'e ya da yeniden tıklanana kadar açık kalır */
  export function painter(lock = false) {
    if (caret.painter && !lock) return stopPainter();
    restoreRange();
    readCaret();
    const el = caretElement();
    if (!el || !editor) return;
    let back = "";
    for (let n: Element | null = el; n && n !== editor; n = n.parentElement) {
      const bg = getComputedStyle(n).backgroundColor;
      if (bg && bg !== "transparent" && bg !== "rgba(0, 0, 0, 0)") {
        back = bg;
        break;
      }
    }
    paint = { bold: caret.bold, italic: caret.italic, underline: caret.underline, strike: caret.strike, fore: getComputedStyle(el).color, back, family: caret.family, size: caret.size };
    caret = { ...caret, painter: lock ? 2 : 1 };
  }
  function applyPaint() {
    const p = paint;
    if (!p) return;
    document.execCommand("removeFormat");
    for (const [cmd, want] of [["bold", p.bold], ["italic", p.italic], ["underline", p.underline], ["strikeThrough", p.strike]] as [string, boolean][]) {
      if (document.queryCommandState(cmd) !== want) document.execCommand(cmd);
    }
    document.execCommand("styleWithCSS", false, "true");
    document.execCommand("foreColor", false, p.fore);
    if (p.back) document.execCommand("hiliteColor", false, p.back);
    document.execCommand("styleWithCSS", false, "false");
    if (p.family) document.execCommand("fontName", false, p.family);
    if (p.size) setFontSize(p.size);
    sync();
    if (caret.painter !== 2) stopPainter();
    readCaret();
  }

  // ---------------------------------------------------------------- tablo
  function cellAt(): HTMLTableCellElement | null {
    const cell = caretElement()?.closest<HTMLTableCellElement>("td,th");
    return cell && editor?.contains(cell) ? cell : null;
  }
  function caretIn(cell: HTMLElement) {
    const r = document.createRange();
    r.selectNodeContents(cell);
    r.collapse(true);
    const sel = window.getSelection();
    sel?.removeAllRanges();
    sel?.addRange(r);
  }
  /** İmlecin bulunduğu tabloda satır/sütun ekleme-silme, stil, gölgelendirme, genişlik */
  export function tableOp(op: TableOp, arg = "") {
    restoreRange();
    const cell = cellAt();
    const sel = window.getSelection();
    if (!cell || !sel?.rangeCount) return;
    if (op === "shade") {
      // Seçim birden çok hücreye yayılmışsa hepsi boyanır
      const r = sel.getRangeAt(0);
      const cells = r.collapsed ? [cell] : Array.from(cell.closest("table")!.querySelectorAll<HTMLTableCellElement>("td,th")).filter((c) => r.intersectsNode(c));
      for (const c of cells) c.style.backgroundColor = arg;
    } else {
      const table = cell.closest("table")!;
      const after = document.createRange();
      after.setStartAfter(table);
      after.collapse(true);
      const next = runTableOp(cell, op, arg);
      if (next) caretIn(next);
      else {
        sel.removeAllRanges();
        sel.addRange(after);
      }
    }
    sync();
    readCaret();
  }

  /** İmlecin (seçimin başının) içinde olduğu öğe */
  function caretElement(): Element | null {
    const sel = window.getSelection();
    if (!editor || !sel || !sel.rangeCount) return null;
    const r = sel.getRangeAt(0);
    let n: Node | null = r.startContainer;
    if (!editor.contains(n)) return null;
    if (!r.collapsed && r.commonAncestorContainer.nodeType === 1) {
      // Seçimin başı çoğu kez biçim öğesinin hemen dışında kalır: seçili ilk metnin biçimi gösterilir
      const walker = document.createTreeWalker(r.commonAncestorContainer, NodeFilter.SHOW_TEXT);
      for (let x = walker.nextNode() as Text | null; x; x = walker.nextNode() as Text | null) {
        if (!r.intersectsNode(x)) continue;
        if ((x === r.endContainer ? r.endOffset : x.length) > (x === r.startContainer ? r.startOffset : 0)) {
          n = x;
          break;
        }
      }
    }
    if (n && n.nodeType !== 1) n = n.parentNode;
    return n as Element | null;
  }

  /** İmleçteki biçim: yazı tipi, punto, açık olan biçimler, paragraf stili, tablo/bağlantı içinde mi */
  function readCaret() {
    const el = caretElement();
    if (!el || !editor) return;
    const ed = editor;
    const cs = getComputedStyle(el);
    const on = (cmd: string) => {
      try {
        return document.queryCommandState(cmd);
      } catch {
        return false;
      }
    };
    const inside = (selector: string) => {
      const m = el.closest(selector);
      return m && m !== ed && ed.contains(m) ? m : null;
    };
    const align = cs.textAlign.replace("-webkit-", "");
    const next: CaretState = {
      family: (cs.fontFamily.split(",")[0] ?? "").trim().replace(/^["']|["']$/g, ""),
      size: Math.round(parseFloat(cs.fontSize) * 0.75 * 2) / 2,
      bold: on("bold"),
      italic: on("italic"),
      underline: on("underline"),
      strike: on("strikeThrough"),
      sub: on("subscript"),
      sup: on("superscript"),
      ul: on("insertUnorderedList"),
      ol: on("insertOrderedList"),
      align: align === "center" || align === "justify" ? align : align === "right" || align === "end" ? "right" : "left",
      block: inside("h1,h2,h3,pre,blockquote:not([style])")?.tagName.toLowerCase() ?? "div",
      link: !!inside("a[href]"),
      table: !!inside("td,th"),
      painter: caret.painter,
    };
    if ((Object.keys(next) as (keyof CaretState)[]).some((k) => next[k] !== caret[k])) caret = next;
  }
  $effect(() => {
    const h = () => readCaret();
    document.addEventListener("selectionchange", h);
    return () => document.removeEventListener("selectionchange", h);
  });
  // Varsayılan yazı tipi değişince (Ayarlar > Tamam) kutular da hemen güncellenir
  $effect(() => {
    void fontStyle;
    const ed = editor;
    if (!ed) return;
    queueMicrotask(() => {
      const cs = getComputedStyle(ed);
      const family = (cs.fontFamily.split(",")[0] ?? "").trim().replace(/^["']|["']$/g, "");
      const size = Math.round(parseFloat(cs.fontSize) * 0.75 * 2) / 2;
      const sel = window.getSelection();
      if (!sel?.rangeCount || !ed.contains(sel.getRangeAt(0).startContainer)) caret = { ...caret, family, size };
      else readCaret();
    });
  });

  /** Outlook "Tablo" ızgarasından: satır × sütun tablo */
  export function insertTable(r: number, c: number) {
    const cell = '<td style="border:1px solid #bfbfbf;padding:3px 6px;min-width:48px;vertical-align:top"><br></td>';
    const row = `<tr>${cell.repeat(c)}</tr>`;
    const html = `<table data-lyn-new style="border-collapse:collapse;border:1px solid #bfbfbf">${row.repeat(r)}</table><div><br></div>`;
    restoreRange();
    if (!document.execCommand("insertHTML", false, html)) {
      // Komut desteklenmiyorsa doğrudan imleç konumuna eklenir
      const sel = window.getSelection();
      if (sel && sel.rangeCount) {
        const range = sel.getRangeAt(0);
        range.deleteContents();
        const frag = range.createContextualFragment(html);
        range.insertNode(frag);
      }
    }
    // Tarayıcı, eklerken sayfanın stiliyle aynı saydığı değerleri (kenarlık çizgisi türü) satır
    // içinden atar; alıcıda o stil olmadığı için kenarlıklar görünmezdi: stil yeniden yazılır
    const fresh = editor?.querySelector<HTMLTableElement>("table[data-lyn-new]");
    if (fresh) {
      fresh.removeAttribute("data-lyn-new");
      applyTableStyle(fresh, "grid");
      const first = fresh.rows[0]?.cells[0];
      if (first) caretIn(first);
    }
    sync();
    readCaret();
  }


  function image(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => exec("insertImage", String(reader.result));
    reader.readAsDataURL(file);
    (e.target as HTMLInputElement).value = "";
  }

  export async function table() {
    const spec = await app.prompt(t("editor.insert_table"), t("editor.table_size"), "3x3");
    if (!spec) return;
    const [r, c] = spec.toLowerCase().split(/[x×*]/).map((n) => Math.max(1, Math.min(20, Number(n) || 2)));
    insertTable(r, c);
  }

  const fonts = ["Noto Sans", "Arial", "Calibri", "Times New Roman", "Georgia", "Verdana", "Tahoma", "Courier New"];
  const sizes = [8, 9, 10, 10.5, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];
  let fileInput: HTMLInputElement | undefined = $state();

  const IMG_EXT = /\.(png|jpe?g|gif|bmp|webp)$/i;
  const toB64 = (buf: ArrayBuffer) => {
    const bytes = new Uint8Array(buf);
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  };
  /** Panodaki dosya yolları (dosya yöneticisinden "Kopyala": text/uri-list ya da x-special/gnome-copied-files) */
  function clipboardPaths(dt: DataTransfer): string[] {
    const raw = [dt.getData("text/uri-list"), dt.getData("x-special/gnome-copied-files"), dt.getData("text/plain")].find((v) => /^file:\/\//m.test(v ?? "")) ?? "";
    return raw
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l.startsWith("file://"))
      .map((l) => {
        try {
          return decodeURIComponent(new URL(l).pathname);
        } catch {
          return "";
        }
      })
      .filter(Boolean);
  }

  /** Outlook gibi yapıştırma: resim gövdeye resim olarak, diğer dosyalar ek olarak */
  async function onpaste(e: ClipboardEvent) {
    const dt = e.clipboardData;
    if (plain) {
      e.preventDefault();
      const text = dt?.getData("text/plain") ?? "";
      document.execCommand("insertText", false, text);
      return;
    }
    if (!dt) return;
    const paths = clipboardPaths(dt);
    const files = Array.from(dt.files ?? []);
    // Ekran görüntüsü / kopyalanan resim (bitmap)
    const imgItems = Array.from(dt.items ?? []).filter((i) => i.kind === "file" && i.type.startsWith("image/"));
    if (!paths.length && !files.length && !imgItems.length) return;
    // Metin ya da HTML de varsa (ör. web sayfasından kopyalanan içerik) tarayıcıya bırakılır
    if (!paths.length && !files.length && dt.getData("text/html")) return;
    e.preventDefault();
    saveRange();
    if (paths.length) {
      const imgs = paths.filter((p) => IMG_EXT.test(p));
      for (const p of imgs) {
        try {
          exec("insertImage", await api.imageDataUrl(p));
        } catch {
          onfiles?.([p]);
        }
      }
      const others = paths.filter((p) => !IMG_EXT.test(p));
      if (others.length) onfiles?.(others);
      return;
    }
    const blobs = files.length ? files : (imgItems.map((i) => i.getAsFile()).filter(Boolean) as File[]);
    const out: string[] = [];
    for (const f of blobs) {
      if (f.type.startsWith("image/")) {
        const url = await new Promise<string>((res) => {
          const r = new FileReader();
          r.onload = () => res(String(r.result));
          r.readAsDataURL(f);
        });
        exec("insertImage", url);
      } else {
        try {
          out.push(await api.writeTempFile(f.name || "dosya", toB64(await f.arrayBuffer())));
        } catch (err) {
          app.error(err);
        }
      }
    }
    if (out.length) onfiles?.(out);
  }

  export function focus() {
    editor?.focus();
    const sel = window.getSelection();
    if (editor && sel) {
      const r = document.createRange();
      r.setStart(editor, 0);
      r.collapse(true);
      sel.removeAllRanges();
      sel.addRange(r);
    }
  }

  export function pickImage() {
    fileInput?.click();
  }

  export function insertHtml(s: string) {
    exec("insertHTML", s);
  }

  // ---------------------------------------------------------------- bağlantı kartı
  // Ctrl+K ya da Bağlantı düğmesi: seçimin yanında metin ve adres alanlı küçük bir kart açılır;
  // imleç bir bağlantının içindeyse o bağlantı düzenlenir (Aç / Kaldır).
  type LinkCard = { x: number; y: number; text: string; url: string; el: HTMLAnchorElement | null; range: Range; initial: string };
  let linkCard = $state<LinkCard | null>(null);
  /** Yüzen kutunun yeri: seçimin altı, sığmazsa üstü; pencereden taşmaz */
  function place(rect: DOMRect, width: number, height: number, above = false) {
    const x = Math.max(8, Math.min(rect.left, window.innerWidth - width - 8));
    const over = rect.top - height - 8;
    const under = rect.bottom + 8;
    const y = above ? (over >= 8 ? over : under) : under + height <= window.innerHeight - 8 ? under : Math.max(8, over);
    return { x, y };
  }
  function rangeRect(r: Range): DOMRect {
    const first = r.getClientRects()[0] ?? r.getBoundingClientRect();
    // Boş satırdaki imlecin dikdörtgeni olmaz: içinde olduğu öğeninki alınır
    if (first.width || first.height) return first;
    const n = r.startContainer;
    return (n.nodeType === 1 ? (n as Element) : n.parentElement!).getBoundingClientRect();
  }
  /** Kart <body>'ye taşındıktan sonra odak verilir (taşınan öğe odağı yitirir) */
  function focusLater(node: HTMLInputElement) {
    setTimeout(() => node.focus(), 0);
  }
  export function link() {
    if (plain) return;
    restoreRange();
    const sel = window.getSelection();
    if (!sel?.rangeCount) return;
    const range = sel.getRangeAt(0).cloneRange();
    const a = caretElement()?.closest<HTMLAnchorElement>("a[href]") ?? null;
    const el = a && editor?.contains(a) ? a : null;
    const text = (el ? (el.textContent ?? "") : sel.toString()).trim();
    bubble = null;
    linkCard = { ...place(rangeRect(range), 328, 132), text, url: el?.getAttribute("href") ?? "", el, range, initial: text };
  }
  function applyLink() {
    const c = linkCard;
    const url = normalizeUrl(c?.url ?? "");
    if (!c || !url) return;
    linkCard = null;
    const text = c.text.trim() || c.url.trim();
    if (c.el && editor?.contains(c.el)) {
      c.el.setAttribute("href", url);
      if (text !== c.initial) c.el.textContent = text;
      editor.focus();
      sync();
      return;
    }
    select(c.range);
    // Metin değişmediyse seçimin biçimi korunur; değiştiyse ya da seçim yoksa yeni bağlantı yazılır
    if (!c.range.collapsed && text === c.initial) exec("createLink", url);
    else exec("insertHTML", `<a href="${escapeHtml(url)}">${escapeHtml(text)}</a>`);
  }
  function removeLink() {
    const el = linkCard?.el;
    linkCard = null;
    if (!el || !editor?.contains(el)) return;
    const r = document.createRange();
    r.selectNodeContents(el);
    select(r);
    exec("unlink");
  }

  // ---------------------------------------------------------------- seçim araç çubuğu
  // Fareyle metin seçilince seçimin üstünde çıkan küçük araç çubuğu (Word/Outlook mini araç
  // çubuğu). Yazmaya başlayınca, seçim kalkınca, kaydırınca ya da dışarı tıklanınca kaybolur.
  let bubble = $state<{ x: number; y: number } | null>(null);
  let dragging = false;
  function showBubble() {
    const sel = window.getSelection();
    if (plain || linkCard || app.setting("compose_mini_toolbar", "1") === "0" || !editor || !sel?.rangeCount || sel.isCollapsed || !editor.contains(sel.anchorNode)) {
      bubble = null;
      return;
    }
    bubble = place(rangeRect(sel.getRangeAt(0)), 372, 34, true);
  }
  $effect(() => {
    // Yüzen kutuların kendisi ve onlardan açılan paletler "dışarısı" sayılmaz
    const FLOATING = "[data-editor-float], [data-slot$='-content'], [data-select-popup]";
    const down = (e: MouseEvent) => {
      const tg = e.target as Element;
      if (tg.closest?.(FLOATING)) return;
      dragging = !!editor?.contains(tg);
      bubble = null;
      linkCard = null;
    };
    const up = () => {
      if (!dragging) return;
      dragging = false;
      // Seçim fare bırakıldıktan sonra kesinleşir
      setTimeout(() => {
        const sel = window.getSelection();
        if (!editor || !sel?.rangeCount || sel.isCollapsed || !editor.contains(sel.anchorNode)) return;
        if (paint) applyPaint();
        else showBubble();
      }, 0);
    };
    document.addEventListener("mousedown", down, true);
    document.addEventListener("mouseup", up);
    return () => {
      document.removeEventListener("mousedown", down, true);
      document.removeEventListener("mouseup", up);
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (plain) return;
    const ctrl = e.ctrlKey || e.metaKey;
    if (e.key === "Escape") {
      // Açık olan yüzen kutu ya da biçim boyacısı kapanır (pencere kapanmaz)
      if (!bubble && !caret.painter) return;
      e.preventDefault();
      bubble = null;
      stopPainter();
    } else if (ctrl && !e.shiftKey && e.key.toLowerCase() === "k") {
      e.preventDefault();
      link();
    } else if (ctrl && (e.key === ">" || e.key === "<")) {
      e.preventDefault();
      stepFontSize(e.key === ">" ? 1 : -1);
    } else if (e.key === "Tab" && !ctrl && !e.altKey) {
      const cell = cellAt();
      if (cell) {
        // Tabloda sonraki/önceki hücre; son hücrede yeni satır açılır
        e.preventDefault();
        const cells = Array.from(cell.closest("table")!.querySelectorAll<HTMLTableCellElement>("td,th"));
        const next = cells[cells.indexOf(cell) + (e.shiftKey ? -1 : 1)];
        if (next) caretIn(next);
        else if (!e.shiftKey) {
          const last = cell.closest("table")!.rows;
          caretIn(last[last.length - 1].cells[cell.cellIndex]);
          tableOp("rowBelow");
          const row = cellAt()?.parentElement as HTMLTableRowElement | undefined;
          if (row) caretIn(row.cells[0]);
        }
      } else if (caret.ul || caret.ol) {
        e.preventDefault();
        exec(e.shiftKey ? "outdent" : "indent");
      } else if (!e.shiftKey) {
        e.preventDefault();
        exec("insertText", "    ");
      }
    } else if (!["Shift", "Control", "Alt", "Meta"].includes(e.key)) bubble = null;
  }

  /** Sözcük ve karakter sayısı (alıntılanan özgün ileti hariç) ile seçili sözcük sayısı */
  export function stats(): { words: number; chars: number; selected: number } {
    let words = 0;
    let chars = 0;
    for (const n of editor?.childNodes ?? []) {
      if (n instanceof HTMLElement && n.matches(".lynora-quote, .mailnix-quote")) continue;
      const s = textStats(n instanceof HTMLElement ? n.innerText : (n.textContent ?? ""));
      words += s.words;
      chars += s.chars;
    }
    const sel = window.getSelection();
    const selected = editor && sel?.rangeCount && !sel.isCollapsed && editor.contains(sel.anchorNode) ? textStats(sel.toString()).words : 0;
    return { words, chars, selected };
  }

  /** Araç çubuklarına verilen komut arayüzü */
  const self: EditorApi = { exec, setFontSize, stepFontSize, setColor, setBlock, setLineHeight, changeCase, link, painter, tableOp };

  // ---------------------------------------------------------------- bul ve değiştir
  /** Düzenleyicideki metin ve her metin düğümünün bu metindeki başlangıcı (biçimlendirme
      bir sözcüğü bölse de bulunur) */
  function textIndex() {
    const nodes: { node: Text; start: number }[] = [];
    let text = "";
    if (editor) {
      const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        if (n.parentElement?.closest("style, script")) continue;
        nodes.push({ node: n as Text, start: text.length });
        text += n.nodeValue ?? "";
      }
    }
    return { nodes, text };
  }

  /** Büyük/küçük harf eşitleme; uzunluk korunur ki bulunan yer metinle aynı kalsın (İ → i) */
  function fold(s: string, caseSensitive: boolean): string {
    if (caseSensitive) return s;
    let out = "";
    for (const c of s) {
      const l = c.toLocaleLowerCase("tr");
      out += l.length === c.length ? l : c;
    }
    return out;
  }

  function rangeAt(nodes: { node: Text; start: number }[], from: number, to: number): Range {
    const locate = (pos: number, end: boolean) => {
      for (let i = nodes.length - 1; i >= 0; i--) {
        const n = nodes[i];
        if (end ? n.start < pos : n.start <= pos) return { node: n.node, offset: pos - n.start };
      }
      return { node: nodes[0].node, offset: 0 };
    };
    const a = locate(from, false);
    const b = locate(to, true);
    const r = document.createRange();
    r.setStart(a.node, a.offset);
    r.setEnd(b.node, b.offset);
    return r;
  }

  function select(r: Range) {
    editor?.focus();
    const sel = window.getSelection();
    sel?.removeAllRanges();
    sel?.addRange(r);
    (r.startContainer.parentElement as HTMLElement | null)?.scrollIntoView({ block: "nearest" });
  }

  /** Seçimin metin içindeki bitiş konumu: arama buradan sürer */
  function selectionEnd(nodes: { node: Text; start: number }[]): number {
    const sel = window.getSelection();
    if (!sel?.rangeCount) return 0;
    const r = sel.getRangeAt(0);
    return (nodes.find((x) => x.node === r.endContainer)?.start ?? -r.endOffset) + r.endOffset;
  }

  /** Sonraki eşleşmeyi seçer; metnin sonuna gelince baştan arar */
  export function findNext(query: string, caseSensitive = false): boolean {
    const { nodes, text } = textIndex();
    if (!query || !nodes.length) return false;
    const hay = fold(text, caseSensitive);
    const q = fold(query, caseSensitive);
    let at = hay.indexOf(q, selectionEnd(nodes));
    if (at < 0) at = hay.indexOf(q);
    if (at < 0) return false;
    select(rangeAt(nodes, at, at + q.length));
    return true;
  }

  /** Seçili eşleşmeyi değiştirip sonrakine geçer; seçim eşleşmiyorsa önce onu bulur */
  export function replaceCurrent(query: string, replacement: string, caseSensitive = false): boolean {
    const current = window.getSelection()?.toString() ?? "";
    if (fold(current, caseSensitive) !== fold(query, caseSensitive)) return findNext(query, caseSensitive);
    document.execCommand("insertText", false, replacement);
    sync();
    findNext(query, caseSensitive);
    return true;
  }

  /** Tüm eşleşmeleri değiştirir; sondan başa gider ki önceki konumlar bozulmasın. Her adımda
      metin yeniden taranır: değiştirme metin düğümlerini bölüp birleştirebilir */
  export function replaceAll(query: string, replacement: string, caseSensitive = false): number {
    if (!query) return 0;
    const q = fold(query, caseSensitive);
    let count = 0;
    let limit = Infinity;
    for (;;) {
      const { nodes, text } = textIndex();
      const at = nodes.length ? fold(text, caseSensitive).lastIndexOf(q, limit) : -1;
      if (at < 0) break;
      select(rangeAt(nodes, at, at + q.length));
      document.execCommand("insertText", false, replacement);
      count++;
      if (at < q.length) break;
      limit = at - q.length;
    }
    if (count) sync();
    return count;
  }
</script>

<!-- Paragraf stilleri (Başlık 1, Alıntı, Kod …): gönderirken aynı kurallar satır içine yazılır -->
<svelte:element this={"style"}>{blockStyleCss(".rich-editor")}</svelte:element>

<div class="flex min-h-0 flex-1 flex-col">
  <input bind:this={fileInput} type="file" accept="image/*" class="hidden" onchange={image} />
  {#if !plain && toolbar}
    <div class="flex flex-wrap items-center gap-0.5 border-b px-2 py-1" role="toolbar">
      <EditorTools layout="bar" editor={self} {caret} />
      <span class="bg-border mx-0.5 h-4 w-px"></span>
      <button type="button" class="ed-btn" data-active={caret.link} title={t("editor.insert_link")} onmousedown={(e) => { e.preventDefault(); link(); }}><LinkIcon class="size-4" /></button>
      <button type="button" class="ed-btn" title={t("editor.insert_image")} onclick={() => fileInput?.click()}><ImageIcon class="size-4" /></button>
      <button type="button" class="ed-btn" title={t("editor.insert_table")} onmousedown={(e) => { e.preventDefault(); table(); }}><TableIcon class="size-4" /></button>
      <button type="button" class="ed-btn" title={t("editor.hr")} onmousedown={(e) => { e.preventDefault(); exec("insertHorizontalRule"); }}><Minus class="size-4" /></button>
      <span class="bg-border mx-0.5 h-4 w-px"></span>
      <button type="button" class="ed-btn" title={t("editor.undo")} onmousedown={(e) => { e.preventDefault(); exec("undo"); }}><Undo class="size-4" /></button>
      <button type="button" class="ed-btn" title={t("editor.redo")} onmousedown={(e) => { e.preventDefault(); exec("redo"); }}><Redo class="size-4" /></button>
    </div>
  {/if}
  <div
    bind:this={editor}
    contenteditable="true"
    role="textbox"
    tabindex="0"
    aria-multiline="true"
    spellcheck={app.setting("spell_check", "1") === "1" ? "true" : "false"}
    class="rich-editor min-h-0 flex-1 overflow-auto px-4 py-3 outline-none {plain ? 'font-mono whitespace-pre-wrap' : ''} {caret.painter ? 'painting' : ''}"
    style="{fontStyle}{zoom !== 1 ? `;zoom:${zoom}` : ''}"
    oninput={sync}
    onblur={flush}
    onscroll={() => (bubble = null)}
    {onkeydown}
    {onpaste}
  ></div>
</div>

{#if bubble}
  <div use:portal data-editor-float="bubble" role="toolbar" aria-label={t("editor.mini_toolbar")} class="bg-popover text-popover-foreground fixed z-[80] flex items-center gap-0.5 rounded-md border p-1 shadow-lg" style="left:{bubble.x}px;top:{bubble.y}px">
    <EditorTools layout="mini" editor={self} {caret} />
  </div>
{/if}

{#if linkCard}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    use:portal
    data-editor-float="link"
    class="bg-popover text-popover-foreground fixed z-[80] flex w-[328px] flex-col gap-1.5 rounded-md border p-2.5 text-[12px] shadow-lg"
    style="left:{linkCard.x}px;top:{linkCard.y}px"
    onsubmit={(e) => { e.preventDefault(); applyLink(); }}
    onkeydown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); linkCard = null; editor?.focus(); } }}
  >
    <label class="flex items-center gap-2"><span class="w-10 shrink-0">{t("editor.link_text")}</span>
      <input class="focus:border-primary h-7 min-w-0 flex-1 rounded border bg-transparent px-1.5 outline-none" bind:value={linkCard.text} />
    </label>
    <label class="flex items-center gap-2"><span class="w-10 shrink-0">{t("editor.link_url")}</span>
      <input class="focus:border-primary h-7 min-w-0 flex-1 rounded border bg-transparent px-1.5 outline-none" placeholder="https://" spellcheck="false" use:focusLater bind:value={linkCard.url} />
    </label>
    <div class="mt-0.5 flex items-center gap-1">
      {#if linkCard.el}
        <button type="button" class="hover:bg-accent flex h-6 items-center gap-1 rounded px-1.5" onclick={() => linkCard?.url && void api.openUrl(normalizeUrl(linkCard.url))}><ExternalLink class="size-3.5" />{t("common.open")}</button>
        <button type="button" class="hover:bg-accent flex h-6 items-center gap-1 rounded px-1.5" onclick={removeLink}><Unlink class="size-3.5" />{t("editor.unlink")}</button>
      {/if}
      <span class="flex-1"></span>
      <button type="button" class="hover:bg-accent h-6 rounded border px-2.5" onclick={() => { linkCard = null; editor?.focus(); }}>{t("common.cancel")}</button>
      <button type="submit" class="bg-primary text-primary-foreground h-6 rounded px-2.5 disabled:opacity-40" disabled={!linkCard.url.trim()}>{t("common.ok")}</button>
    </div>
  </form>
{/if}

<style>
  :global(.rich-editor) {
    font-family: "Noto Sans", Calibri, Arial, sans-serif;
    font-size: 11pt;
    line-height: 1.35;
    /* Bulanık/ titrek yazıyı önler (optimizeLegibility her tuşta satırları yeniden
       şekillendirdiği için kullanılmaz) */
    -webkit-font-smoothing: antialiased;
    contain: content;
  }
  /* Biçim Boyacısı açıkken imleç */
  :global(.rich-editor.painting) {
    cursor: copy;
  }
  :global(.rich-editor p) {
    margin: 0;
  }
  /* Alıntılanan özgün iletideki alıntı blokları (kullanıcının "Alıntı" stili editor.ts'ten gelir) */
  :global(.rich-editor :is(.lynora-quote, .mailnix-quote) blockquote:not([style])) {
    border-left: 3px solid #c7c7c7;
    margin: 0 0 0 4px;
    padding-left: 10px;
    color: #555;
  }
  :global(.rich-editor ul) {
    list-style: disc;
    padding-left: 24px;
  }
  :global(.rich-editor ol) {
    list-style: decimal;
    padding-left: 24px;
  }
  :global(.rich-editor a) {
    color: #1a73e8;
    text-decoration: underline;
  }
  /* Tailwind'in sıfırlaması resmi blok yapar: satır içi resim (imza, ifade) satırda kalır */
  :global(.rich-editor img) {
    display: inline;
    vertical-align: baseline;
    max-width: 100%;
  }
  :global(.rich-editor table img) {
    max-width: none;
  }
  /* Alıntılanan tablolar okuma bölmesindeki gibi görünür: Tailwind'in sıfırlaması
     (border:0, border-collapse:collapse) border/cellspacing özniteliklerini ezer.
     Satır içi stiller (Word tabloları) bunların üstündedir. */
  :global(.rich-editor table) {
    border-collapse: separate;
    text-indent: initial;
  }
  :global(.rich-editor table[border]:not([border="0"])) {
    border: 1px outset gray;
  }
  :global(.rich-editor table[border]:not([border="0"]) > * > tr > :is(td, th)),
  :global(.rich-editor table[border]:not([border="0"]) > tr > :is(td, th)) {
    border: 1px inset gray;
  }
  :global(.rich-editor hr) {
    margin: 8px 0;
  }
</style>
