// Güvenilir göndericiler: bu adreslerden ("ali@sirket.com") ya da alan adlarından ("@sirket.com")
// gelen iletilerdeki uzak resimler sorulmadan indirilir. Alan adı alt alan adlarını da kapsar
// ("@sirket.com" → "ali@posta.sirket.com"; "ali@kotusirket.com" kapsanmaz). Yalnızca bu bilgisayarda saklanır.
import { app } from "$lib/stores/app.svelte";

// Herkese açık posta servisleri: alan adına güvenmek herkese güvenmek olur, yalnızca adres önerilir
const PUBLIC_DOMAINS = new Set([
  "gmail.com", "googlemail.com", "outlook.com", "outlook.com.tr", "hotmail.com", "hotmail.com.tr", "live.com",
  "msn.com", "windowslive.com", "yahoo.com", "yahoo.com.tr", "ymail.com", "icloud.com", "me.com", "mac.com",
  "aol.com", "yandex.com", "yandex.com.tr", "yandex.ru", "mail.ru", "proton.me", "protonmail.com", "gmx.com",
  "gmx.de", "gmx.net", "zoho.com", "mynet.com", "superonline.com", "ttmail.com",
]);

/** Girişi kayıt biçimine çevirir: "ali@sirket.com" ya da "@sirket.com". "sirket.com", "*.sirket.com",
 *  "*@sirket.com" ve "Ali <ali@sirket.com>" de kabul edilir; geçersizse null. */
export function normalizeTrustEntry(entry: string): string | null {
  let v = entry.trim().toLowerCase();
  const angle = /<([^<>]*)>/.exec(v);
  if (angle) v = angle[1].trim();
  v = v.replace(/^mailto:/, "").replace(/^\*@/, "@").replace(/^\*?\.+/, "");
  const at = v.lastIndexOf("@");
  const local = at < 0 ? "" : v.slice(0, at);
  const domain = v.slice(at + 1).replace(/\.$/, "");
  if (/[\s@<>,;"]/.test(local)) return null;
  if (!/^[\p{L}\p{N}-]+(\.[\p{L}\p{N}-]+)+$/u.test(domain)) return null;
  return `${local}@${domain}`;
}

/** Adres listedeki bir adresle ya da alan adıyla (alt alan adları dahil) eşleşiyor mu */
export function matchesTrusted(addr: string, entries: string[]): boolean {
  const sender = normalizeTrustEntry(addr);
  if (!sender || sender.startsWith("@")) return false;
  const domain = sender.slice(sender.lastIndexOf("@") + 1);
  return entries.some((e) => {
    const entry = normalizeTrustEntry(e);
    if (!entry) return false;
    if (!entry.startsWith("@")) return entry === sender;
    const d = entry.slice(1);
    return domain === d || domain.endsWith(`.${d}`);
  });
}

/** Göndericinin alan adı ("@sirket.com"); herkese açık posta servisiyse null */
export function trustDomainOf(addr: string): string | null {
  const v = normalizeTrustEntry(addr);
  if (!v || v.startsWith("@")) return null;
  const domain = v.slice(v.lastIndexOf("@") + 1);
  return PUBLIC_DOMAINS.has(domain) ? null : `@${domain}`;
}

export function loadTrustedSenders(): string[] {
  try {
    const v = JSON.parse(app.setting("trusted_senders", "[]"));
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function isTrustedSender(addr: string): boolean {
  return matchesTrusted(addr, loadTrustedSenders());
}

/** Listeye ekler ve kayıt biçimini döndürür (zaten varsa dokunmaz); geçersiz girişte null */
export async function addTrustedSender(entry: string): Promise<string | null> {
  const v = normalizeTrustEntry(entry);
  if (!v) return null;
  const list = loadTrustedSenders();
  if (!list.some((x) => normalizeTrustEntry(x) === v)) await app.setSetting("trusted_senders", JSON.stringify([...list, v]));
  return v;
}

export async function removeTrustedSender(entry: string) {
  await app.setSetting("trusted_senders", JSON.stringify(loadTrustedSenders().filter((x) => x !== entry)));
}
