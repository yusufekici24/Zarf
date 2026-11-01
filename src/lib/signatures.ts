// İmzalar (Outlook "İmzalar ve Kırtasiye"): yalnızca bu bilgisayarda saklanır, sunucuya
// (Exchange) gönderilmez. Her hesap için yeni iletilerde ve yanıtlarda/iletmelerde
// kullanılacak varsayılan imza ayrı seçilir.
import { app } from "$lib/stores/app.svelte";
import { textToHtml } from "$lib/format";

export type Signature = { name: string; html: string };

const asHtml = (s: string) => (s.trim().startsWith("<") ? s : textToHtml(s));

/** Kayıtlı imzalar; ilk kullanımda eski (hesaba bağlı) imzalar hesap adıyla listelenir */
export function loadSignatures(): Signature[] {
  const raw = app.setting("signatures", "");
  if (raw) {
    try {
      const v = JSON.parse(raw) as Signature[];
      if (Array.isArray(v)) return v.filter((s) => s && typeof s.name === "string");
    } catch {
      /* bozuk kayıt */
    }
  }
  return app.accounts.filter((a) => a.signature?.trim()).map((a) => ({ name: a.email, html: asHtml(a.signature) }));
}

export async function saveSignatures(list: Signature[]) {
  await app.setSetting("signatures", JSON.stringify(list));
}

/** Hesabın varsayılan imzasının adı ("" = imza yok). Seçilmemişse eski hesap imzası. */
export function defaultSignature(accountId: number | null, kind: "new" | "reply"): string {
  if (accountId == null) return "";
  const v = app.settings[`sig_${kind}_${accountId}`];
  if (v !== undefined) return v;
  const a = app.accounts.find((x) => x.id === accountId);
  return a?.signature?.trim() ? a.email : "";
}

/** İletiye eklenecek imza bloğu */
export function signatureBlock(name: string): string {
  const s = loadSignatures().find((x) => x.name === name);
  return s && s.html.trim() ? `<div class="lynora-signature">${s.html}</div>` : "";
}
