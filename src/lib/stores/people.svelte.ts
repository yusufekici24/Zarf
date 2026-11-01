// Adı olmayan adreslerin görünen adları (dizinden, toplu ve önbellekli) ve
// kişi kartı (Outlook'ta ada tıklayınca açılan kart) durumu.
import { api } from "$lib/api";
import { SvelteMap } from "svelte/reactivity";

class People {
  /** e-posta (küçük harf) → görünen ad ("" = bulunamadı) */
  names = new SvelteMap<string, string>();
  private pending = new Set<string>();
  private timer: ReturnType<typeof setTimeout> | null = null;

  /** Görünen ad; henüz bilinmiyorsa arka planda sorulur */
  name(email: string): string {
    const e = email.trim().toLowerCase();
    if (!e.includes("@")) return "";
    const n = this.names.get(e);
    if (n === undefined && !this.pending.has(e)) {
      this.pending.add(e);
      if (!this.timer) this.timer = setTimeout(() => this.flush(), 30);
    }
    return n ?? "";
  }

  private flush() {
    this.timer = null;
    const list = [...this.pending];
    this.pending.clear();
    for (let i = 0; i < list.length; i += 100) {
      const part = list.slice(i, i + 100);
      api
        .displayNames(part)
        .then((m) => part.forEach((e) => this.names.set(e, m[e] ?? "")))
        .catch(() => part.forEach((e) => this.names.set(e, "")));
    }
  }

  /** Açık kişi kartı */
  card = $state<{ email: string; name: string; x: number; y: number } | null>(null);

  openCard(entry: string, ev: MouseEvent) {
    const m = entry.match(/<([^>]+)>/);
    const email = (m ? m[1] : entry).trim();
    const name = (m ? entry.slice(0, entry.indexOf("<")) : "").trim().replace(/^"|"$/g, "");
    // Aynı ada yeniden tıklamak kartı kapatır
    if (this.card && this.card.email.toLowerCase() === email.toLowerCase()) {
      this.card = null;
      return;
    }
    const r = (ev.currentTarget as HTMLElement | null)?.getBoundingClientRect();
    this.card = { email, name: name || this.name(email), x: r ? r.left : ev.clientX, y: r ? r.bottom + 4 : ev.clientY };
  }
}

export const people = new People();
