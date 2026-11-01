// Dosya aç / kaydet / klasör seç diyaloğu (eklentiyle aynı imza). Linux'ta kendi komutumuz
// (filedialog.rs): masaüstünün dosya seçici servisi varsa diyalog ayrı süreçte açılır, ağ
// konumu ya da dosya yöneticisi takılsa bile uygulama donmaz. Desteklenmezse eklentiye düşer.
import { invoke } from "@tauri-apps/api/core";
import * as plugin from "@tauri-apps/plugin-dialog";

type Filter = { name: string; extensions: string[] };
type SaveOptions = { title?: string; defaultPath?: string; filters?: Filter[] };
type OpenOptions = { title?: string; defaultPath?: string; filters?: Filter[]; multiple?: boolean; directory?: boolean };

async function native(mode: "save" | "open" | "folder", o: { title?: string; defaultPath?: string; filters?: Filter[]; multiple?: boolean }): Promise<string[] | null | undefined> {
  try {
    return await invoke<string[] | null>("file_dialog", { mode, title: o.title ?? null, defaultPath: o.defaultPath ?? null, filters: o.filters ?? [], multiple: !!o.multiple });
  } catch {
    return undefined; // komut yok/desteklenmiyor: eklenti kullanılır
  }
}

export async function save(o: SaveOptions = {}): Promise<string | null> {
  const r = await native("save", o);
  if (r === undefined) return plugin.save(o);
  return r?.[0] ?? null;
}

export async function open(o: OpenOptions = {}): Promise<string | string[] | null> {
  const r = await native(o.directory ? "folder" : "open", o);
  if (r === undefined) return plugin.open(o as Parameters<typeof plugin.open>[0]) as Promise<string | string[] | null>;
  if (!r || !r.length) return null;
  return o.multiple ? r : r[0];
}
