// Arama kutusunun sözdizimi: anahtar sözcükler (kimden:, konu:, tarih: …), yazılan son parçanın
// çözülmesi ve öneriler. Sorguyu asıl çözen arka uçtur (db.rs: parse_query); buradaki tablo
// onunla aynı anahtarları taşır.

export type OpKind = "person" | "text" | "enum" | "date";
export interface Operator {
  /** Arka uçtaki asıl ad */
  id: string;
  tr: string;
  en: string;
  kind: OpKind;
  /** Bu adlarla da yazılabilir (katlanmış) */
  alias: string[];
  /** Önerilen değerler: [tr, en] */
  values?: [string, string][];
}

const DATES: [string, string][] = [
  ["bugün", "today"],
  ["dün", "yesterday"],
  ["bu hafta", "this week"],
  ["geçen hafta", "last week"],
  ["bu ay", "this month"],
  ["geçen ay", "last month"],
  ["bu yıl", "this year"],
];

export const OPERATORS: Operator[] = [
  { id: "from", tr: "kimden", en: "from", kind: "person", alias: ["gonderen"] },
  { id: "to", tr: "kime", en: "to", kind: "person", alias: ["alici"] },
  { id: "cc", tr: "bilgi", en: "cc", kind: "person", alias: [] },
  { id: "bcc", tr: "gizli", en: "bcc", kind: "person", alias: [] },
  { id: "participants", tr: "kişi", en: "participants", kind: "person", alias: ["participant", "katilimci"] },
  { id: "subject", tr: "konu", en: "subject", kind: "text", alias: [] },
  { id: "body", tr: "gövde", en: "body", kind: "text", alias: ["metin"] },
  { id: "attachment", tr: "ekadı", en: "attachmentname", kind: "text", alias: ["filename", "dosya"] },
  { id: "category", tr: "kategori", en: "category", kind: "enum", alias: ["categories"] },
  { id: "has", tr: "ek", en: "has", kind: "enum", alias: ["hasattachment", "hasattachments", "ekvar", "attachment", "attachments"], values: [["var", "attachment"], ["yok", "no"]] },
  {
    id: "is", tr: "durum", en: "is", kind: "enum", alias: [],
    values: [["okunmamış", "unread"], ["okunmuş", "read"], ["bayraklı", "flagged"], ["önemli", "important"], ["yanıtlandı", "replied"], ["yanıtlanmadı", "unreplied"], ["taslak", "draft"], ["sabit", "pinned"]],
  },
  { id: "importance", tr: "önem", en: "importance", kind: "enum", alias: [], values: [["yüksek", "high"], ["normal", "normal"], ["düşük", "low"]] },
  { id: "on", tr: "tarih", en: "date", kind: "date", alias: ["on", "received", "sent"], values: [...DATES, ["7g", "7d"], ["30g", "30d"]] },
  { id: "after", tr: "sonra", en: "after", kind: "date", alias: ["since"], values: DATES },
  { id: "before", tr: "önce", en: "before", kind: "date", alias: [], values: DATES },
  { id: "folder", tr: "klasör", en: "folder", kind: "enum", alias: ["in"], values: [["gelen", "inbox"], ["gönderilmiş", "sent"], ["taslak", "drafts"], ["arşiv", "archive"], ["silinmiş", "trash"]] },
];

/** Aramadaki katlama: büyük/küçük harf, Türkçe İ/ı ve aksan farkı gözetilmez */
export function fold(s: string): string {
  return s.toLocaleLowerCase("tr").normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/ı/g, "i");
}

export function operator(key: string): Operator | undefined {
  const k = fold(key);
  return OPERATORS.find((o) => fold(o.tr) === k || o.en === k || o.id === k || o.alias.includes(k));
}

/** Değer, gerekiyorsa tırnak içinde */
export function quote(v: string): string {
  const c = v.trim().replace(/"/g, "");
  return /[\s()]/.test(c) ? `"${c}"` : c;
}

export interface Part {
  /** Parçanın metindeki yeri */
  start: number;
  end: number;
  raw: string;
  /** Baştaki "-" (DEĞİL) */
  neg: boolean;
  /** Anahtarın yazıldığı hali ("Kimden") ve tanınan anahtar */
  keyText?: string;
  op?: Operator;
  value: string;
}

/** Sorguyu parçalara ayırır: tırnak içi tek parça; "kimden: ali" (boşluklu) tek parçadır */
export function parts(query: string): Part[] {
  const out: Part[] = [];
  const re = /(?:[^\s"]|"[^"]*"?)+/g;
  for (let m = re.exec(query); m; m = re.exec(query)) {
    const raw = m[0];
    const prev = out[out.length - 1];
    // Önceki parça değersiz bir anahtarsa ("kimden:") bu parça onun değeridir
    if (prev?.op && !prev.value && prev.raw.endsWith(":") && !/^(OR|AND|NOT|VEYA|VE|DEĞİL)$/.test(raw)) {
      prev.end = m.index + raw.length;
      prev.raw = query.slice(prev.start, prev.end);
      prev.value = raw.replace(/"/g, "");
      continue;
    }
    const neg = /^-[^\s]/.test(raw);
    const body = neg ? raw.slice(1) : raw;
    const km = body.match(/^([^\s:@"()]+):(.*)$/);
    const op = km ? operator(km[1]) : undefined;
    out.push({ start: m.index, end: m.index + raw.length, raw, neg, keyText: op ? km![1] : undefined, op, value: (op ? km![2] : body).replace(/"/g, "") });
  }
  return out;
}

/** İmlecin (metnin sonunun) bulunduğu, yazılmakta olan parça */
export function current(query: string): Part | undefined {
  if (!query || /\s$/.test(query)) {
    // "kimden: " — anahtar yazıldı, değer bekleniyor
    const last = parts(query).pop();
    return last?.op && !last.value ? { ...last, end: query.length } : undefined;
  }
  return parts(query).pop();
}

export interface Hint {
  /** Kutuda görünen metin ("kimden:", "bu hafta") */
  label: string;
  /** Yazılan parçanın yerine konacak metin */
  insert: string;
  /** Açıklama için anahtar (operatör önerisi) */
  op?: Operator;
  /** Seçilince arama hemen yapılır (değer tamam) */
  done: boolean;
}

/**
 * Yazılan parçaya göre öneriler: anahtar adı ("kim" → "kimden:") ya da anahtarın değerleri
 * ("durum:ok" → "okunmamış"). Kişi anahtarlarında değer önerisi adres defterinden gelir (burada değil).
 */
export function hints(query: string, lang: string, categories: string[] = []): Hint[] {
  const p = current(query);
  if (!p) return [];
  const tr = lang === "tr";
  if (!p.op) {
    const w = fold(p.value);
    if (!w || p.raw.includes('"')) return [];
    return OPERATORS.filter((o) => [o.tr, o.en, ...o.alias].some((n) => fold(n).startsWith(w)))
      .slice(0, 5)
      .map((o) => ({ label: `${tr ? o.tr : o.en}:`, insert: `${p.neg ? "-" : ""}${tr ? o.tr : o.en}:`, op: o, done: false }));
  }
  const values = p.op.id === "category" ? categories.map((c) => [c, c] as [string, string]) : (p.op.values ?? []);
  const w = fold(p.value);
  return values
    .filter(([a, b]) => fold(a).startsWith(w) || fold(b).startsWith(w))
    .filter(([a, b]) => fold(a) !== w && fold(b) !== w)
    .slice(0, 8)
    .map(([a, b]) => {
      const v = tr ? a : b;
      return { label: v, insert: `${p.neg ? "-" : ""}${p.keyText}:${quote(v)} `, done: true };
    });
}

/** Yazılan parçayı `insert` ile değiştirir */
export function replaceCurrent(query: string, insert: string): string {
  const p = current(query);
  return p ? query.slice(0, p.start) + insert : query + insert;
}
