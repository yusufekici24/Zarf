// Güvenilir göndericiler: bu adreslerden ("ali@sirket.com") ya da alan adlarından ("@sirket.com")
// gelen iletilerdeki uzak resimler sorulmadan indirilir. Yalnızca bu bilgisayarda saklanır.
import { app } from "$lib/stores/app.svelte";

export function loadTrustedSenders(): string[] {
  try {
    const v = JSON.parse(app.setting("trusted_senders", "[]"));
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function isTrustedSender(addr: string): boolean {
  const sender = addr.trim().toLowerCase();
  if (!sender) return false;
  const domain = sender.slice(sender.lastIndexOf("@"));
  return loadTrustedSenders().some((e) => {
    const entry = e.trim().toLowerCase();
    return entry === sender || entry === domain;
  });
}

export async function addTrustedSender(entry: string) {
  const v = entry.trim().toLowerCase();
  if (!v.includes("@")) return;
  const list = loadTrustedSenders();
  if (!list.some((x) => x.toLowerCase() === v)) await app.setSetting("trusted_senders", JSON.stringify([...list, v]));
}

export async function removeTrustedSender(entry: string) {
  await app.setSetting("trusted_senders", JSON.stringify(loadTrustedSenders().filter((x) => x !== entry)));
}
