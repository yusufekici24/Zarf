// Tarih/saat ve metin biçimlendirme yardımcıları (seçili dile göre).
import { i18n, t } from "./i18n.svelte";

export const DAY = 86400;

export function toDate(ts: number): Date {
  return new Date(ts * 1000);
}

export function toTs(d: Date): number {
  return Math.floor(d.getTime() / 1000);
}

export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function addDays(d: Date, n: number): Date {
  const r = new Date(d);
  r.setDate(r.getDate() + n);
  return r;
}

export function addMonths(d: Date, n: number): Date {
  const r = new Date(d.getFullYear(), d.getMonth() + n, 1);
  const last = new Date(r.getFullYear(), r.getMonth() + 1, 0).getDate();
  r.setDate(Math.min(d.getDate(), last));
  return r;
}

/** Pazartesi ile başlayan hafta */
export function startOfWeek(d: Date, firstDay = 1): Date {
  const s = startOfDay(d);
  const diff = (s.getDay() - firstDay + 7) % 7;
  return addDays(s, -diff);
}

export function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

export function fmtTime(ts: number | Date): string {
  const d = ts instanceof Date ? ts : toDate(ts);
  return d.toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit" });
}

export function fmtDate(ts: number | Date, opts: Intl.DateTimeFormatOptions = { dateStyle: "medium" }): string {
  const d = ts instanceof Date ? ts : toDate(ts);
  return d.toLocaleDateString(i18n.locale, opts);
}

export function fmtDateLong(ts: number | Date): string {
  return fmtDate(ts, { weekday: "long", day: "numeric", month: "long", year: "numeric" });
}

export function fmtDateTime(ts: number | Date): string {
  const d = ts instanceof Date ? ts : toDate(ts);
  return d.toLocaleString(i18n.locale, { dateStyle: "medium", timeStyle: "short" });
}

/** İleti listesi: bugünse saat, bu haftaysa gün+saat, değilse tarih. */
export function fmtListDate(ts: number): string {
  if (!ts) return "";
  const d = toDate(ts);
  const now = new Date();
  if (sameDay(d, now)) return fmtTime(d);
  const diff = (startOfDay(now).getTime() - startOfDay(d).getTime()) / 86400000;
  if (diff > 0 && diff < 7) return d.toLocaleDateString(i18n.locale, { weekday: "short" }) + " " + fmtTime(d);
  if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString(i18n.locale, { day: "numeric", month: "short" });
  return d.toLocaleDateString(i18n.locale);
}

/** İleti listesi gruplama başlığı (Bugün, Dün, Bu hafta, ...) */
export function dateGroup(ts: number): string {
  const d = startOfDay(toDate(ts));
  const today = startOfDay(new Date());
  const diff = Math.round((today.getTime() - d.getTime()) / 86400000);
  if (diff <= 0) return t("group.today");
  if (diff === 1) return t("group.yesterday");
  if (diff < 7 && d >= startOfWeek(today)) return t("group.this_week");
  if (diff < 14) return t("group.last_week");
  if (d.getFullYear() === today.getFullYear() && d.getMonth() === today.getMonth()) return t("group.this_month");
  return t("group.older");
}

export function fmtSize(n: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${i === 0 ? n : n.toFixed(1)} ${units[i]}`;
}

export function monthName(m: number): string {
  return new Date(2024, m, 1).toLocaleDateString(i18n.locale, { month: "long" });
}

export function weekdayShort(d: Date): string {
  return d.toLocaleDateString(i18n.locale, { weekday: "short" });
}

export function weekdayLong(d: Date): string {
  return d.toLocaleDateString(i18n.locale, { weekday: "long" });
}

// ---------------------------------------------------------------- adresler
export function splitNameAddr(s: string): { name: string; addr: string } {
  const m = /^\s*"?(.*?)"?\s*<([^<>]+)>\s*$/.exec(s);
  if (m) return { name: m[1].trim(), addr: m[2].trim() };
  if (s.includes("@")) return { name: "", addr: s.trim() };
  return { name: s.trim(), addr: "" };
}

export function displayName(s: string): string {
  const { name, addr } = splitNameAddr(s);
  return name || addr || s;
}

// Outlook'un tanıdığı bozuk/özel adres biçimleri (arka uçtaki mail::address ile aynı kurallar):
// akıllı tırnaklar, görünmez karakterler, MIME kodlu adlar, çift/kapanmamış köşeli parantez,
// mailto:/SMTP: önekleri, Exchange iç adresleri (IMCEAEX, /O=… DN, X500:)
const QUOTES = "\"'‘’‚‛“”„‟«»‹›`´＂＇′″";
const LIST_QUOTES = "\"“”„«»＂";
const isQuote = (c: string) => QUOTES.includes(c);

export function sanitizeAddressText(s: string): string {
  return s
    .replace(/[​-‏‪-‮⁠-⁤⁦-⁩﻿­᠎]/g, "")
    .replace(/[   -  　\t\r]/g, " ")
    .replace(/＠/g, "@")
    .replace(/＜/g, "<")
    .replace(/＞/g, ">")
    .replace(/，/g, ",")
    .replace(/；/g, ";");
}

export function decodeMimeWords(s: string): string {
  if (!s.includes("=?")) return s;
  return s
    .replace(/(=\?[^?\s]+\?[bBqQ]\?[^?\s]*\?=)\s+(?==\?[^?\s]+\?[bBqQ]\?)/g, "$1")
    .replace(/=\?([^?\s]+)\?([bBqQ])\?([^?\s]*)\?=/g, (all, cs: string, enc: string, data: string) => {
      try {
        let bytes: Uint8Array;
        if (enc.toLowerCase() === "b") {
          const bin = atob(data);
          bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
        } else {
          const out: number[] = [];
          for (let i = 0; i < data.length; i++) {
            const c = data[i];
            if (c === "_") out.push(32);
            else if (c === "=" && i + 2 < data.length && /^[0-9a-f]{2}$/i.test(data.slice(i + 1, i + 3))) {
              out.push(parseInt(data.slice(i + 1, i + 3), 16));
              i += 2;
            } else out.push(c.charCodeAt(0));
          }
          bytes = Uint8Array.from(out);
        }
        return new TextDecoder(cs.split("*")[0]).decode(bytes);
      } catch {
        return all;
      }
    });
}

export function isExchangeDn(a: string): boolean {
  return /^\/(o|cn)=/i.test(a.trim());
}

function imceaexToDn(local: string): string {
  return local.replace(/\+([0-9a-f]{2})|_/gi, (m, h) => (h ? String.fromCharCode(parseInt(h, 16)) : "/"));
}

function trimChars(s: string, pred: (c: string) => boolean): string {
  let a = 0;
  let b = s.length;
  while (a < b && pred(s[a])) a++;
  while (b > a && pred(s[b - 1])) b--;
  return s.slice(a, b);
}

export function cleanEmail(raw: string): string {
  let a = trimChars(raw, (c) => isQuote(c) || /\s/.test(c) || "<>()[],;".includes(c));
  for (;;) {
    const m = /^(mailto:|smtp:|x500:|ex:|sip:)/i.exec(a);
    if (!m) break;
    a = trimChars(a.slice(m[0].length), (c) => isQuote(c) || /\s/.test(c));
  }
  if (isExchangeDn(a)) return a.trim();
  a = [...a].filter((c) => !/\s/.test(c) && !isQuote(c)).join("").replace(/[.:]+$/, "");
  if (/^imceaex-/i.test(a) && a.includes("@")) return imceaexToDn(a.slice(8, a.lastIndexOf("@")));
  return a;
}

function cleanName(raw: string): string {
  return trimChars(raw.split(/\s+/).filter(Boolean).join(" "), (c) => isQuote(c) || /\s/.test(c));
}

/** Tek alıcıyı "Ad <adres>" (ya da yalnızca adres) biçimine getirir */
export function cleanAddress(entry: string): string {
  const s = decodeMimeWords(sanitizeAddressText(entry)).trim();
  if (!s) return "";
  let name = "";
  let email = "";
  const lt = s.indexOf("<");
  if (lt >= 0) {
    const inner = s.slice(lt).replace(/^<+/, "").split(">")[0];
    name = s.slice(0, lt);
    email = cleanEmail(inner);
  } else {
    const bare = cleanEmail(s);
    const words = trimChars(s, (c) => isQuote(c) || /\s/.test(c)).split(/\s+/).filter(Boolean).length;
    if ((bare.includes("@") && words <= 1) || isExchangeDn(bare)) email = bare;
    else {
      const tok = s.split(/\s+/).reverse().find((w) => w.includes("@"));
      if (tok) {
        name = s.slice(0, s.lastIndexOf(tok));
        email = cleanEmail(tok);
      } else name = s;
    }
  }
  name = cleanName(decodeMimeWords(name));
  if (!email) return name;
  if (name.toLowerCase() === email.toLowerCase() || cleanEmail(name).toLowerCase() === email.toLowerCase()) name = "";
  if (!name) return email;
  return /[,;"<>@()]/.test(name) ? `"${name.replace(/"/g, "'")}" <${email}>` : `${name} <${email}>`;
}

function splitRaw(text: string, quotes: boolean): { parts: string[]; balanced: boolean } {
  const out: string[] = [];
  let cur = "";
  let q = false;
  let angle = false;
  let unbalanced = false;
  for (const c of text) {
    if (quotes && LIST_QUOTES.includes(c) && !angle) q = !q;
    if (c === "<") angle = true;
    if (c === ">") angle = false;
    if (c === "\n") {
      if (q) unbalanced = true;
      q = false;
      angle = false;
      if (cur.trim()) out.push(cur.trim());
      cur = "";
    } else if ((c === "," || c === ";") && !q && !angle) {
      if (cur.trim()) out.push(cur.trim());
      cur = "";
    } else cur += c;
  }
  if (cur.trim()) out.push(cur.trim());
  return { parts: out, balanced: !q && !unbalanced };
}

/** Adres listesini böler ve her alıcıyı temizler (adressiz yalnızca ad girişleri korunur) */
export function splitAddresses(text: string): string[] {
  const t = sanitizeAddressText(text);
  const first = splitRaw(t, true);
  const parts = first.balanced ? first.parts : splitRaw(t, false).parts;
  return parts.map(cleanAddress).filter((x) => x.trim());
}

/**
 * Outlook gibi baş harfler: ad ve soyadın ilk harfi. Unvan/birim gibi ekler
 * ("Ali Veli (Bilgi İşlem)", "Ali Veli - BİDB", "[Kurum] Ali Veli")
 * ve "Erdem, Yusuf" biçimi dikkate alınır.
 */
export function initials(name: string): string {
  let s = (name || "").replace(/\([^)]*\)|\[[^\]]*\]|\{[^}]*\}|<[^>]*>|"/g, " ");
  s = s.split(/\s+[-–—|/]\s+|\s*\|\s*/)[0];
  const comma = s.split(",");
  if (comma.length === 2 && comma[0].trim() && comma[1].trim()) s = `${comma[1]} ${comma[0]}`;
  else s = comma[0];
  // Yalnızca e-posta adresi: yerel kısım (ad.soyad)
  if (s.includes("@") && !s.trim().includes(" ")) s = s.split("@")[0].replace(/[._-]+/g, " ");
  const parts = s
    .split(/[\s_]+/)
    .map((p) => p.replace(/^[^\p{L}\p{N}]+/u, ""))
    .filter((p) => /^\p{L}/u.test(p));
  if (!parts.length) return "?";
  // Kurumsal görünen adlardaki birim kısaltması ("Ali Veli BT", "Ayşe Kaya İK"):
  // üç ve daha fazla sözcükte sondaki en çok 3 harflik BÜYÜK HARF sözcük soyadı değildir
  while (parts.length >= 3) {
    const last = parts[parts.length - 1];
    const prev = parts[parts.length - 2];
    const isAbbr = [...last].length <= 3 && last === last.toLocaleUpperCase("tr") && last !== last.toLocaleLowerCase("tr");
    const prevMixed = prev !== prev.toLocaleUpperCase("tr");
    if (isAbbr && prevMixed) parts.pop();
    else break;
  }
  const first = (w: string) => [...w][0] ?? "";
  if (parts.length === 1) return [...parts[0]].slice(0, 2).join("").toLocaleUpperCase("tr");
  return (first(parts[0]) + first(parts[parts.length - 1])).toLocaleUpperCase("tr");
}

const AVATAR_COLORS = ["#1e88e5", "#43a047", "#e53935", "#8e24aa", "#fb8c00", "#00897b", "#3949ab", "#6d4c41", "#d81b60", "#546e7a"];

export function avatarColor(name: string): string {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return AVATAR_COLORS[h % AVATAR_COLORS.length];
}

export function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

export function textToHtml(s: string): string {
  return escapeHtml(s)
    .replace(/(https?:\/\/[^\s<]+)/g, '<a href="$1">$1</a>')
    .replace(/\n/g, "<br>");
}

/** <input type="datetime-local"> değerleri */
export function toInputDate(ts: number | null | undefined): string {
  if (!ts) return "";
  const d = toDate(ts);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function toInputTime(ts: number | null | undefined): string {
  if (!ts) return "";
  const d = toDate(ts);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function fromInputs(date: string, time = "00:00"): number | null {
  if (!date) return null;
  const [y, m, d] = date.split("-").map(Number);
  const [hh, mm] = (time || "00:00").split(":").map(Number);
  return toTs(new Date(y, m - 1, d, hh || 0, mm || 0));
}

/** Aramada karşılaştırma: küçük harf, Türkçe i/ı/İ farkı gözetilmez ("KALAMIŞ" ~ "Kalamış") */
export function fold(s: string): string {
  return (s || "").toLowerCase().replace(/\u0307/g, "").replace(/ı/g, "i");
}
