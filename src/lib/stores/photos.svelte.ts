// Gönderen fotoğrafları: bellek önbelleği + aynı anda en fazla 3 istek.
import { invoke } from "@tauri-apps/api/core";
import { SvelteMap } from "svelte/reactivity";

class PhotoStore {
  /** e-posta → veri adresi (null: fotoğraf yok) */
  cache = new SvelteMap<string, string | null>();
  private queue: string[] = [];
  private running = 0;
  private requested = new Set<string>();

  get(email: string): string | null | undefined {
    const e = email.trim().toLowerCase();
    if (!e.includes("@")) return null;
    if (!this.requested.has(e)) {
      this.requested.add(e);
      this.queue.push(e);
      this.pump();
    }
    return this.cache.get(e);
  }

  private pump() {
    while (this.running < 3 && this.queue.length) {
      const e = this.queue.shift()!;
      this.running++;
      invoke<string | null>("sender_photo", { email: e })
        .then((url) => this.cache.set(e, url ?? null))
        .catch(() => this.cache.set(e, null))
        .finally(() => {
          this.running--;
          this.pump();
        });
    }
  }
}

export const photos = new PhotoStore();
