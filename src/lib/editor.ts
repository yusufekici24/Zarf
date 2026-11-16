// Biçimlendirmeli düzenleyicinin (RichEditor) ortak tanımları: imleç durumu, komut arayüzü, renk
// paletleri, paragraf ve tablo stilleri, harf dönüşümü, sözcük sayımı. Ayrıntı: docs/yazma-penceresi.md

/** İmlecin bulunduğu yerdeki biçim: şerit ve araç çubuğu düğmeleri bunu gösterir */
export interface CaretState {
  family: string;
  size: number;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  sub: boolean;
  sup: boolean;
  ul: boolean;
  ol: boolean;
  align: "left" | "center" | "right" | "justify";
  /** Paragraf stili: div (normal), h1, h2, h3, blockquote, pre */
  block: string;
  link: boolean;
  table: boolean;
  /** Biçim boyacısı: 0 kapalı, 1 bir kez, 2 kilitli (çift tıklama) */
  painter: number;
}

export const caretState = (): CaretState => ({
  family: "", size: 0, bold: false, italic: false, underline: false, strike: false, sub: false, sup: false,
  ul: false, ol: false, align: "left", block: "div", link: false, table: false, painter: 0,
});

export type ColorKind = "fore" | "back" | "shade";
export type CaseMode = "sentence" | "lower" | "upper" | "title";
export type TableOp = "rowAbove" | "rowBelow" | "colLeft" | "colRight" | "delRow" | "delCol" | "delTable" | "style" | "shade" | "fit" | "auto";

/** Araç çubuklarının düzenleyiciden beklediği komutlar (RichEditor bileşeni bunları dışa verir) */
export interface EditorApi {
  exec(cmd: string, value?: string): void;
  setFontSize(pt: number): void;
  stepFontSize(dir: number): void;
  /** Boş renk: yazıda otomatik, vurguda ve gölgelendirmede renk yok */
  setColor(kind: ColorKind, color: string): void;
  setBlock(tag: string): void;
  setLineHeight(value: string): void;
  changeCase(mode: CaseMode): void;
  link(): void | Promise<void>;
  painter(lock?: boolean): void;
  tableOp(op: TableOp, arg?: string): void;
}

// ---------------------------------------------------------------- renkler
/** Office tema renkleri: her sütun taban renk ve beş tonu */
export const THEME_COLORS: string[][] = [
  ["#ffffff", "#f2f2f2", "#d8d8d8", "#bfbfbf", "#a5a5a5", "#7f7f7f"],
  ["#000000", "#7f7f7f", "#595959", "#3f3f3f", "#262626", "#0c0c0c"],
  ["#e7e6e6", "#d0cece", "#aeabab", "#757070", "#3a3838", "#171616"],
  ["#44546a", "#d6dce4", "#adb9ca", "#8496b0", "#323f4f", "#222a35"],
  ["#4472c4", "#d9e2f3", "#b4c6e7", "#8eaadb", "#2f5496", "#1f3864"],
  ["#ed7d31", "#fbe5d5", "#f7cbac", "#f4b183", "#c55a11", "#833c0b"],
  ["#a5a5a5", "#ededed", "#dbdbdb", "#c9c9c9", "#7b7b7b", "#525252"],
  ["#ffc000", "#fff2cc", "#fee599", "#ffd965", "#bf9000", "#7f6000"],
  ["#5b9bd5", "#deebf6", "#bdd7ee", "#9cc3e5", "#2e75b5", "#1e4e79"],
  ["#70ad47", "#e2efd9", "#c5e0b3", "#a8d08d", "#538135", "#375623"],
];
export const STANDARD_COLORS = ["#c00000", "#ff0000", "#ffc000", "#ffff00", "#92d050", "#00b050", "#00b0f0", "#0070c0", "#002060", "#7030a0"];
/** Word'ün on beş vurgu rengi */
export const HIGHLIGHT_COLORS = ["#ffff00", "#00ff00", "#00ffff", "#ff00ff", "#0000ff", "#ff0000", "#000080", "#008080", "#008000", "#800080", "#800000", "#808000", "#808080", "#c0c0c0", "#000000"];

/** "#abc", "abc123", "#ABC123" → "#abc123"; geçersizse boş */
export function normalizeHex(v: string): string {
  const s = v.trim().replace(/^#/, "").toLowerCase();
  if (/^[0-9a-f]{3}$/.test(s)) return "#" + [...s].map((c) => c + c).join("");
  return /^[0-9a-f]{6}$/.test(s) ? "#" + s : "";
}

// ---------------------------------------------------------------- paragraf stilleri
/** Stil galerisi. `css` hem düzenleyicide (üretilen kural) hem gönderilen iletide (satır içi) kullanılır */
export const BLOCK_STYLES: { tag: string; label: string; css: string }[] = [
  { tag: "div", label: "editor.style_normal", css: "" },
  { tag: "h1", label: "editor.style_h1", css: "font-size:20pt;font-weight:normal;color:#2f5496;margin:10pt 0 3pt;line-height:1.2" },
  { tag: "h2", label: "editor.style_h2", css: "font-size:15pt;font-weight:normal;color:#2f5496;margin:8pt 0 2pt;line-height:1.25" },
  { tag: "h3", label: "editor.style_h3", css: "font-size:12pt;font-weight:bold;color:#1f3763;margin:6pt 0 2pt;line-height:1.3" },
  { tag: "blockquote", label: "editor.style_quote", css: "margin:4pt 0 4pt 4px;padding:2px 0 2px 10px;border-left:3px solid #c7c7c7;color:#555555" },
  { tag: "pre", label: "editor.style_code", css: "font-family:Consolas,'Courier New',monospace;font-size:10pt;background:#f3f3f3;border:1px solid #e1e1e1;border-radius:3px;padding:6px 8px;margin:4pt 0;white-space:pre-wrap" },
];
/** Stil yalnızca kullanıcının verdiği bloklara uygulanır: alıntılanan özgün iletideki başlıklara ve
    girintinin ürettiği (kendi satır içi stili olan) blockquote'lara dokunulmaz */
const QUOTED = ".lynora-quote *, .mailnix-quote *";
const styleSelector = (tag: string) => `${tag}:not([style]):not(${QUOTED})`;

/** Düzenleyicide görünen stil kuralları (`scope`: düzenleyicinin seçicisi) */
export function blockStyleCss(scope: string): string {
  return BLOCK_STYLES.filter((s) => s.css).map((s) => `${scope} ${styleSelector(s.tag)}{${s.css}}`).join("\n");
}

/** Gönderilen HTML: alıcıda stil sayfamız olmadığı için paragraf stilleri satır içine yazılır */
export function inlineBlockStyles(root: HTMLElement) {
  for (const s of BLOCK_STYLES) {
    if (s.css) root.querySelectorAll<HTMLElement>(styleSelector(s.tag)).forEach((el) => el.setAttribute("style", s.css));
  }
}

// ---------------------------------------------------------------- tablo stilleri
export interface TableStyle {
  key: string;
  label: string;
  /** Kenarlıklar: all (ızgara), rows (yalnızca yatay), none */
  lines: "all" | "rows" | "none";
  line: string;
  head?: { bg: string; fg: string };
  band?: string;
}
export const TABLE_STYLES: TableStyle[] = [
  { key: "grid", label: "table.style_grid", lines: "all", line: "#bfbfbf" },
  { key: "blue", label: "table.style_blue", lines: "all", line: "#8eaadb", head: { bg: "#4472c4", fg: "#ffffff" }, band: "#d9e2f3" },
  { key: "green", label: "table.style_green", lines: "all", line: "#a8d08d", head: { bg: "#70ad47", fg: "#ffffff" }, band: "#e2efd9" },
  { key: "gray", label: "table.style_gray", lines: "all", line: "#c9c9c9", head: { bg: "#e7e6e6", fg: "#000000" } },
  { key: "lines", label: "table.style_lines", lines: "rows", line: "#7f7f7f", head: { bg: "", fg: "" } },
  { key: "plain", label: "table.style_plain", lines: "none", line: "" },
];

// ---------------------------------------------------------------- metin
const SENTENCE = /(^|[.!?…]\s+)(\p{L})/gu;
const WORD = /(^|[^\p{L}\p{N}'’])(\p{L})/gu;

/** Harf dönüşümü (Word "Büyük/Küçük Harf Değiştir"); `locale` tr ise i/İ ve ı/I doğru çevrilir */
export function changeCase(text: string, mode: CaseMode, locale: string): string {
  const lower = text.toLocaleLowerCase(locale);
  if (mode === "lower") return lower;
  if (mode === "upper") return text.toLocaleUpperCase(locale);
  return lower.replace(mode === "title" ? WORD : SENTENCE, (_m, pre: string, ch: string) => pre + ch.toLocaleUpperCase(locale));
}

export function textStats(text: string): { words: number; chars: number } {
  const t = text.replace(/ /g, " ").trim();
  return { words: t ? t.split(/\s+/).length : 0, chars: t.replace(/\s/g, "").length };
}

/** Bağlantı adresi: şeması olmayan adrese https://, e-posta adresine mailto: eklenir */
export function normalizeUrl(v: string): string {
  const s = v.trim();
  if (!s) return "";
  if (/^[a-z][a-z0-9+.-]*:/i.test(s)) return s;
  if (/^[^\s@/]+@[^\s@/]+\.[^\s@/]+$/.test(s)) return `mailto:${s}`;
  return `https://${s}`;
}

/** Svelte eylemi: öğeyi <body>'ye taşır (kırpan/dönüştüren üst öğelerden etkilenmeyen yüzen kutular) */
export function portal(node: HTMLElement) {
  document.body.appendChild(node);
  return { destroy: () => node.remove() };
}
