<script lang="ts">
  // Biçimlendirmeli e-posta düzenleyicisi (contenteditable + biçim araç çubuğu).
  import { t } from "$lib/i18n.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import Bold from "@lucide/svelte/icons/bold";
  import Italic from "@lucide/svelte/icons/italic";
  import Underline from "@lucide/svelte/icons/underline";
  import Strikethrough from "@lucide/svelte/icons/strikethrough";
  import List from "@lucide/svelte/icons/list";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import AlignLeft from "@lucide/svelte/icons/align-left";
  import AlignCenter from "@lucide/svelte/icons/align-center";
  import AlignRight from "@lucide/svelte/icons/align-right";
  import Indent from "@lucide/svelte/icons/indent-increase";
  import Outdent from "@lucide/svelte/icons/indent-decrease";
  import LinkIcon from "@lucide/svelte/icons/link";
  import ImageIcon from "@lucide/svelte/icons/image";
  import TableIcon from "@lucide/svelte/icons/table";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Undo from "@lucide/svelte/icons/undo-2";
  import Redo from "@lucide/svelte/icons/redo-2";
  import Minus from "@lucide/svelte/icons/minus";
  import Highlighter from "@lucide/svelte/icons/highlighter";
  import Baseline from "@lucide/svelte/icons/baseline";

  let {
    html = $bindable(""),
    plain = false,
    toolbar = true,
    onchange,
    fontStyle = "",
    caret = $bindable({ family: "", size: 0 }),
    onfiles,
  }: {
    html?: string;
    plain?: boolean;
    toolbar?: boolean;
    onchange?: () => void;
    /** Varsayılan yazı tipi (Kırtasiye ve Yazı Tipleri) */
    fontStyle?: string;
    /** İmlecin bulunduğu yerdeki yazı tipi ve punto (şeritteki kutular bunu gösterir, Outlook gibi) */
    caret?: { family: string; size: number };
    /** Yapıştırılan resim olmayan dosyalar (yol listesi): ek olarak eklenir */
    onfiles?: (paths: string[]) => void;
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
    const h = () => saveRange();
    document.addEventListener("selectionchange", h);
    return () => document.removeEventListener("selectionchange", h);
  });
  function restoreRange() {
    if (!editor) return;
    editor.focus();
    const sel = window.getSelection();
    if (!sel) return;
    if (sel.rangeCount && editor.contains(sel.anchorNode)) return;
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
    document.execCommand("styleWithCSS", false, "false");
    document.execCommand("fontSize", false, "7");
    editor.querySelectorAll('font[size="7"]').forEach((f) => {
      const span = document.createElement("span");
      span.style.fontSize = `${pt}pt`;
      while (f.firstChild) span.appendChild(f.firstChild);
      f.replaceWith(span);
    });
    sync();
    readCaret();
  }

  /** İmleçteki yazı tipi: ilk aile adı ve punto */
  function readCaret() {
    const sel = window.getSelection();
    if (!editor || !sel || !sel.rangeCount) return;
    let n: Node | null = sel.getRangeAt(0).startContainer;
    if (!editor.contains(n)) return;
    if (n && n.nodeType !== 1) n = n.parentNode;
    const cs = getComputedStyle(n as Element);
    const family = (cs.fontFamily.split(",")[0] ?? "").trim().replace(/^["']|["']$/g, "");
    const size = Math.round((parseFloat(cs.fontSize) * 0.75) * 2) / 2;
    if (family !== caret.family || size !== caret.size) caret = { family, size };
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
      if (!sel?.rangeCount || !ed.contains(sel.getRangeAt(0).startContainer)) caret = { family, size };
      else readCaret();
    });
  });

  /** Outlook "Tablo" ızgarasından: satır × sütun tablo */
  export function insertTable(r: number, c: number) {
    const cell = '<td style="border:1px solid #bfbfbf;padding:3px 6px;min-width:48px;vertical-align:top"><br></td>';
    const row = `<tr>${cell.repeat(c)}</tr>`;
    const html = `<table style="border-collapse:collapse;border:1px solid #bfbfbf">${row.repeat(r)}</table><div><br></div>`;
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
    sync();
  }

  export async function link() {
    const url = await app.prompt(t("editor.insert_link"), t("editor.url"), "https://");
    if (url) exec("createLink", url);
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

<div class="flex min-h-0 flex-1 flex-col">
  <input bind:this={fileInput} type="file" accept="image/*" class="hidden" onchange={image} />
  {#if !plain && toolbar}
    <div class="flex flex-wrap items-center gap-0.5 border-b px-2 py-1">
      <select class="h-7 rounded border bg-transparent px-1 text-xs" title={t("editor.font")} value={caret.family} onchange={(e) => exec("fontName", (e.target as HTMLSelectElement).value)}>
        {#if caret.family && !fonts.includes(caret.family)}<option value={caret.family}>{caret.family}</option>{/if}
        {#each fonts as f (f)}<option value={f}>{f}</option>{/each}
      </select>
      <select class="h-7 rounded border bg-transparent px-1 text-xs" title={t("editor.size")} value={String(caret.size)} onchange={(e) => setFontSize(Number((e.target as HTMLSelectElement).value))}>
        {#if caret.size && !sizes.includes(caret.size)}<option value={String(caret.size)}>{caret.size}</option>{/if}
        {#each sizes as v (v)}<option value={String(v)}>{v}</option>{/each}
      </select>
      <span class="bg-border mx-1 h-5 w-px"></span>
      {#each ([[Bold, "bold", "editor.bold"], [Italic, "italic", "editor.italic"], [Underline, "underline", "editor.underline"], [Strikethrough, "strikeThrough", "editor.strike"]] as [typeof Bold, string, string][]) as [Icon, cmd, label] (cmd)}
        <button type="button" class="hover:bg-accent rounded p-1.5" title={t(label)} onmousedown={(e) => { e.preventDefault(); exec(cmd); }}><Icon class="size-4" /></button>
      {/each}
      <label class="hover:bg-accent relative flex cursor-pointer items-center rounded p-1.5" title={t("editor.color")}>
        <Baseline class="size-4" />
        <input type="color" class="absolute inset-0 cursor-pointer opacity-0" value="#c00000" onchange={(e) => exec("foreColor", (e.target as HTMLInputElement).value)} />
      </label>
      <label class="hover:bg-accent relative flex cursor-pointer items-center rounded p-1.5" title={t("editor.highlight")}>
        <Highlighter class="size-4" />
        <input type="color" class="absolute inset-0 cursor-pointer opacity-0" value="#ffff00" onchange={(e) => exec("hiliteColor", (e.target as HTMLInputElement).value)} />
      </label>
      <span class="bg-border mx-1 h-5 w-px"></span>
      {#each ([[List, "insertUnorderedList", "editor.bullets"], [ListOrdered, "insertOrderedList", "editor.numbering"], [Outdent, "outdent", "editor.outdent"], [Indent, "indent", "editor.indent"], [AlignLeft, "justifyLeft", "editor.left"], [AlignCenter, "justifyCenter", "editor.center"], [AlignRight, "justifyRight", "editor.right"]] as [typeof Bold, string, string][]) as [Icon, cmd, label] (cmd)}
        <button type="button" class="hover:bg-accent rounded p-1.5" title={t(label)} onmousedown={(e) => { e.preventDefault(); exec(cmd); }}><Icon class="size-4" /></button>
      {/each}
      <span class="bg-border mx-1 h-5 w-px"></span>
      <button type="button" class="hover:bg-accent rounded p-1.5" title={t("editor.insert_link")} onmousedown={(e) => { e.preventDefault(); link(); }}><LinkIcon class="size-4" /></button>
      <button type="button" class="hover:bg-accent rounded p-1.5" title={t("editor.insert_image")} onclick={() => fileInput?.click()}><ImageIcon class="size-4" /></button>
      <button type="button" class="hover:bg-accent rounded p-1.5" title={t("editor.insert_table")} onmousedown={(e) => { e.preventDefault(); table(); }}><TableIcon class="size-4" /></button>
      <button type="button" class="hover:bg-accent rounded p-1.5" title={t("editor.hr")} onmousedown={(e) => { e.preventDefault(); exec("insertHorizontalRule"); }}><Minus class="size-4" /></button>
      <button type="button" class="hover:bg-accent rounded p-1.5" title={t("editor.clear_format")} onmousedown={(e) => { e.preventDefault(); exec("removeFormat"); }}><Eraser class="size-4" /></button>
      <span class="bg-border mx-1 h-5 w-px"></span>
      <button type="button" class="hover:bg-accent rounded p-1.5" title={t("editor.undo")} onmousedown={(e) => { e.preventDefault(); exec("undo"); }}><Undo class="size-4" /></button>
      <button type="button" class="hover:bg-accent rounded p-1.5" title={t("editor.redo")} onmousedown={(e) => { e.preventDefault(); exec("redo"); }}><Redo class="size-4" /></button>
    </div>
  {/if}
  <div
    bind:this={editor}
    contenteditable="true"
    role="textbox"
    tabindex="0"
    aria-multiline="true"
    spellcheck={app.setting("spell_check", "1") === "1" ? "true" : "false"}
    class="rich-editor min-h-0 flex-1 overflow-auto px-4 py-3 outline-none {plain ? 'font-mono whitespace-pre-wrap' : ''}"
    style={fontStyle}
    oninput={sync}
    onblur={flush}
    {onpaste}
  ></div>
</div>

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
  :global(.rich-editor p) {
    margin: 0;
  }
  :global(.rich-editor blockquote) {
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
