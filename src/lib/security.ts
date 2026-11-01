// Okuma bölmesindeki güvenlik çubuğunun metni ve durumu.
import { t } from "$lib/i18n.svelte";
import type { SecurityView } from "$lib/types";

/** İmza geçersiz ya da ileti çözülemedi */
export function securityFailed(s: SecurityView): boolean {
  return s.signature === "bad" || (s.encrypted && !s.content);
}

/** İmza varsa güvenilir; imzasız şifreli ileti çözüldüyse sorun yok */
export function securityTrusted(s: SecurityView): boolean {
  return !s.signed || s.signature === "good";
}

export function securityText(s: SecurityView): string {
  const parts: string[] = [];
  if (s.encrypted) parts.push(s.content ? t("sec.decrypted") : t("sec.decrypt_failed", s.error || t("sec.no_secret_key")));
  if (s.signed) {
    const known = ["good", "untrusted", "bad", "no_key"].includes(s.signature);
    parts.push(known ? t(`sec.sig_${s.signature}`, s.signer) : t("sec.sig_error", s.error));
  }
  return `${s.method === "pgp" ? "OpenPGP" : "S/MIME"}: ${parts.join(" · ")}`;
}
