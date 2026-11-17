// "Adım Kime ya da Bilgi satırında" vurgusu (Outlook koşullu biçimlendirmesindeki "adımın bulunduğu yer").
// Seçenekler > Posta > İleti listesi: `me_highlight` (kapalı / Kime / Bilgi / ikisi), `me_color`, `me_names`.
//
// Exchange ileti listesi alıcıları yalnızca görünen adla verir (DisplayTo/DisplayCc); adres ileti açılınca
// MIME'dan yazılır. Bu yüzden hem adres hem ad aranır. Tek sözcüklü ad (ör. "Yusuf") başkalarını da
// yakalayacağı için hesap adından alınmaz; kullanıcı isterse `me_names`'e kendisi yazar.
import { fold } from "$lib/format";
import type { Account } from "$lib/types";

export type MeMode = "off" | "to" | "cc" | "tocc";

/** Vurgu renkleri: anahtar → [ayarlardaki ad anahtarı, liste satırı sınıfı (açık ve koyu görünüm)] */
export const ME_COLORS: Record<string, [string, string]> = {
  red: ["accent.red", "text-[#c50f1f] dark:text-[#ff7b72]"],
  orange: ["accent.orange", "text-[#b54708] dark:text-[#ffa657]"],
  green: ["accent.green", "text-[#107c41] dark:text-[#56d364]"],
  purple: ["accent.purple", "text-[#6b46b8] dark:text-[#bc8cff]"],
  blue: ["accent.blue", "text-[#0f6cbd] dark:text-[#6cb6ff]"],
};

export function meColorClass(key: string): string {
  return (ME_COLORS[key] ?? ME_COLORS.red)[1];
}

const WORD = /[\p{L}\p{N}]/u;

/** Aranacak adresler ve adlar (katlanmış, tekil). Paylaşılan posta kutuları sayılmaz: oradaki her ileti o adrese gelir. */
export function meKeys(accounts: Account[], extra = ""): string[] {
  const keys = new Set<string>();
  for (const a of accounts) {
    if (a.shared_of != null) continue;
    const email = fold(a.email || "").trim();
    if (email.includes("@")) keys.add(email);
    const name = fold(a.display_name || "").trim().replace(/\s+/g, " ");
    if (name.includes(" ") && !name.includes("@")) keys.add(name);
  }
  // Ayırıcı noktalı virgül: Exchange adları "Soyad, Ad" biçiminde virgül içerebilir
  for (const part of extra.split(/[;\n]/)) {
    const k = fold(part).trim().replace(/\s+/g, " ");
    if (k.length >= 3) keys.add(k);
  }
  return [...keys];
}

/** Alıcı metninde (Kime/Bilgi) anahtarlardan biri tam sözcük/adres olarak geçiyor mu */
export function mentionsMe(text: string, keys: string[]): boolean {
  if (!text || !keys.length) return false;
  const h = fold(text).replace(/\s+/g, " ");
  for (const k of keys) {
    const email = k.includes("@");
    // Adreste nokta, tire ve alt çizgi de adresin parçasıdır ("ali@x.com", "veli@x.com.tr" içinde sayılmaz)
    const inner = (c: string | undefined) => !!c && (WORD.test(c) || (email && "._-+".includes(c)));
    for (let i = h.indexOf(k); i >= 0; i = h.indexOf(k, i + 1)) {
      if (!inner(h[i - 1]) && !inner(h[i + k.length])) return true;
    }
  }
  return false;
}

/** İletinin seçilen alanlarında (Kime, Bilgi ya da ikisi) kullanıcının adı/adresi var mı */
export function addressedToMe(m: { to_addr?: string; cc?: string }, mode: string, keys: string[]): boolean {
  if (mode === "to") return mentionsMe(m.to_addr ?? "", keys);
  if (mode === "cc") return mentionsMe(m.cc ?? "", keys);
  if (mode === "tocc") return mentionsMe(m.to_addr ?? "", keys) || mentionsMe(m.cc ?? "", keys);
  return false;
}
