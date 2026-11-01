// Kimlikler (takma adlar): bir hesabın ek "Kimden" adresleri. Her kimliğin kendi görünen adı,
// yanıt adresi (Reply-To) ve varsayılan imzası olabilir.
import { app } from "$lib/stores/app.svelte";

export type Identity = { account_id: number; email: string; name: string; reply_to: string; signature: string };

export function loadIdentities(accountId?: number | null): Identity[] {
  try {
    const v = JSON.parse(app.setting("identities", "[]"));
    if (!Array.isArray(v)) return [];
    const list = v.filter((x) => x && typeof x.email === "string") as Identity[];
    return accountId == null ? list : list.filter((x) => x.account_id === accountId);
  } catch {
    return [];
  }
}

export async function saveIdentities(list: Identity[]) {
  await app.setSetting("identities", JSON.stringify(list));
}

/** Yanıtlanan ileti hangi kimliğe gönderildiyse o seçilir: takma ada gelene takma adla yanıt verilir */
export function identityForReply(accountId: number | null, recipients: string): Identity | null {
  const text = recipients.toLowerCase();
  return loadIdentities(accountId).find((x) => text.includes(x.email.trim().toLowerCase())) ?? null;
}

export function identityByEmail(accountId: number | null, email: string): Identity | null {
  const e = email.trim().toLowerCase();
  return e ? (loadIdentities(accountId).find((x) => x.email.trim().toLowerCase() === e) ?? null) : null;
}
