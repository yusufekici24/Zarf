// Düzenleyicinin geri al / yinele geçmişi (Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z; şeritteki düğmeler).
//
// Tarayıcının kendi geri alma yığını kullanılmaz:
// - WebKitGTK yinelemeyi yalnızca Ctrl+Shift+Z'ye bağlar; Ctrl+Y hiçbir şey yapmıyordu.
// - Satır aralığı, harf dönüşümü, tablo işlemleri ve punto düzeltmesi DOM'u doğrudan değiştirir;
//   tarayıcının yığınına girmez, üstelik sonraki geri almaları da bozar.
// Bunun yerine her adımdan sonra içerik ve seçim (metin konumu olarak) kaydedilir. Arka arkaya yazılan
// harfler tek adımdır: düzenleyici yazarken her tuşta içeriği seri hale getirmez (büyük alıntılı
// yanıtta takılırdı), yazma grubu kapanınca (ara, Enter, yazma ↔ silme geçişi, başka bir komut) bir kez kaydeder.

export type Sel = [number, number];
export interface Snapshot {
  html: string;
  sel: Sel | null;
}

/** Arka arkaya yazma/silme bu süreden kısa aralıklarla gelirse tek adım sayılır */
export const MERGE_MS = 1200;

export class EditHistory {
  private items: Snapshot[] = [];
  private index = -1;
  private chars = 0;

  constructor(
    private maxSteps = 200,
    /** Bütün adımların toplam uzunluğu (karakter): satır içi resimli büyük iletide bellek şişmesin */
    private maxChars = 40_000_000,
  ) {}

  get canUndo(): boolean {
    return this.index > 0;
  }
  get canRedo(): boolean {
    return this.index < this.items.length - 1;
  }
  get size(): number {
    return this.items.length;
  }

  /** Yeni içerikle baştan başlar (pencereye ileti yüklendi) */
  reset(html: string, sel: Sel | null = null) {
    this.items = [{ html, sel }];
    this.index = 0;
    this.chars = html.length;
  }

  /** Değişiklikten sonraki durumu yeni adım olarak kaydeder; içerik aynıysa yalnızca seçim güncellenir */
  record(html: string, sel: Sel | null) {
    if (this.index < 0) return this.reset(html, sel);
    const cur = this.items[this.index];
    if (cur.html === html) {
      cur.sel = sel;
      return;
    }
    // Geri alınmış adımlar (yinelenebilecekler) yeni değişiklikle düşer
    for (const s of this.items.splice(this.index + 1)) this.chars -= s.html.length;
    this.items.push({ html, sel });
    this.chars += html.length;
    this.index++;
    while (this.items.length > 2 && (this.items.length > this.maxSteps || this.chars > this.maxChars)) {
      this.chars -= this.items.shift()!.html.length;
      this.index--;
    }
  }

  /** Bütün adımlarda bir metni değiştirir (yapıştırılan resmin geçici `blob:` adresi → `data:` adresi):
      geri alınınca geçersiz adres geri gelmesin */
  replaceAll(from: string, to: string) {
    if (!from) return;
    for (const s of this.items) {
      if (!s.html.includes(from)) continue;
      const html = s.html.split(from).join(to);
      this.chars += html.length - s.html.length;
      s.html = html;
    }
  }

  undo(): Snapshot | null {
    return this.canUndo ? this.items[--this.index] : null;
  }

  redo(): Snapshot | null {
    return this.canRedo ? this.items[++this.index] : null;
  }
}

/** Girdi olayının türünden yazma grubu: "type" ve "delete" ayrı ayrı birleşir, "" (Enter, yapıştırma,
    biçim …) her zaman ayrı adımdır */
export function inputKind(inputType: string | undefined): string {
  if (!inputType) return "";
  if (inputType === "insertText" || inputType === "insertCompositionText" || inputType === "insertReplacementText") return "type";
  if (inputType.startsWith("deleteContent") || inputType.startsWith("deleteWord")) return "delete";
  return "";
}

/** Açık yazma grubu (tür ve son girdi zamanı) bu girdiden önce kapanmalı mı */
export function closesGroup(open: { kind: string; at: number } | null, kind: string, now = Date.now()): boolean {
  return !!open && (kind === "" || kind !== open.kind || now - open.at >= MERGE_MS);
}

// ------------------------------------------------------------------ seçim ↔ metin konumu
// Konum: düzenleyicideki metin karakterleri; <br> ve <img> birer karakter sayılır (boş satır ve resim
// yanındaki imleç de doğru yere döner).

function isToken(n: Node): boolean {
  return n.nodeType === Node.TEXT_NODE || (n.nodeType === Node.ELEMENT_NODE && ((n as Element).tagName === "BR" || (n as Element).tagName === "IMG"));
}
function tokenLength(n: Node): number {
  return n.nodeType === Node.TEXT_NODE ? (n as Text).length : 1;
}
function tokens(root: Node): Node[] {
  const out: Node[] = [];
  const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, { acceptNode: (n) => (isToken(n) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP) });
  for (let n = w.nextNode(); n; n = w.nextNode()) out.push(n);
  return out;
}

/** Bir sınır noktasının (container, offset) metin konumu */
function offsetOf(root: Node, list: Node[], container: Node, offset: number): number {
  let count = 0;
  if (container.nodeType === Node.TEXT_NODE) {
    for (const n of list) {
      if (n === container) return count + offset;
      count += tokenLength(n);
    }
    return count;
  }
  // Öğe içindeki nokta: offset'teki çocuk ve sonrası sayılmaz
  const stop = container.childNodes[offset] ?? null;
  for (const n of list) {
    if (stop) {
      if (n === stop || stop.contains(n) || stop.compareDocumentPosition(n) & Node.DOCUMENT_POSITION_FOLLOWING) break;
    } else if (!(container === root || container.contains(n) || container.compareDocumentPosition(n) & Node.DOCUMENT_POSITION_PRECEDING)) break;
    count += tokenLength(n);
  }
  return count;
}

export function readSelection(root: HTMLElement): Sel | null {
  const sel = window.getSelection();
  if (!sel?.rangeCount) return null;
  const r = sel.getRangeAt(0);
  if (!root.contains(r.startContainer) || !root.contains(r.endContainer)) return null;
  const list = tokens(root);
  return [offsetOf(root, list, r.startContainer, r.startOffset), offsetOf(root, list, r.endContainer, r.endOffset)];
}

/** Metin konumundaki sınır noktası. Konum bir metnin sonu ile <br> arasına düşerse <br>'den önce
    durulur (boş satıra dönen imleç bir önceki satırın sonuna kaçmasın) */
function pointAt(root: HTMLElement, list: Node[], target: number): [Node, number] {
  let count = 0;
  for (let i = 0; i < list.length; i++) {
    const n = list[i];
    const len = tokenLength(n);
    if (n.nodeType === Node.TEXT_NODE) {
      const next = list[i + 1];
      const atEnd = target === count + len;
      if (target < count + len || (atEnd && !(next && next.nodeType === Node.ELEMENT_NODE))) return [n, target - count];
    } else if (target === count) {
      return [n.parentNode!, Array.prototype.indexOf.call(n.parentNode!.childNodes, n)];
    }
    count += len;
  }
  return [root, root.childNodes.length];
}

export function writeSelection(root: HTMLElement, s: Sel | null) {
  const sel = window.getSelection();
  if (!sel) return;
  const list = tokens(root);
  const r = document.createRange();
  if (!s) {
    r.setStart(root, 0);
    r.collapse(true);
  } else {
    const [a, ao] = pointAt(root, list, s[0]);
    r.setStart(a, ao);
    if (s[1] === s[0]) r.collapse(true);
    else {
      const [b, bo] = pointAt(root, list, s[1]);
      r.setEnd(b, bo);
    }
  }
  sel.removeAllRanges();
  sel.addRange(r);
}
