// İleti şablonları: sık gönderilen içerikler (konu + biçimli gövde). İmzalar gibi yalnızca
// bu bilgisayarda saklanır.
import { app } from "$lib/stores/app.svelte";

export type Template = { name: string; subject: string; html: string };

export function loadTemplates(): Template[] {
  try {
    const v = JSON.parse(app.setting("templates", "[]"));
    return Array.isArray(v) ? v.filter((x) => x && typeof x.name === "string") : [];
  } catch {
    return [];
  }
}

export async function saveTemplates(list: Template[]) {
  await app.setSetting("templates", JSON.stringify(list));
}
