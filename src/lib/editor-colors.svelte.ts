// Düzenleyicinin renk düğmelerinde son kullanılan renkler (yazı, vurgu, hücre gölgesi). Bütün
// düğmeler (şerit, araç çubuğu, seçim araç çubuğu) aynı durumu paylaşır; tarayıcı deposunda kalır.
import type { ColorKind } from "./editor";

const KEY = "lynora.editor.colors";

class EditorColors {
  fore = $state("#c00000");
  back = $state("#ffff00");
  shade = $state("#d9e2f3");
  recent = $state<string[]>([]);

  constructor() {
    try {
      const v = JSON.parse(localStorage.getItem(KEY) ?? "null");
      if (v && typeof v === "object") {
        for (const k of ["fore", "back", "shade"] as const) if (typeof v[k] === "string") this[k] = v[k];
        if (Array.isArray(v.recent)) this.recent = v.recent.filter((c: unknown) => typeof c === "string").slice(0, 10);
      }
    } catch {
      /* depo yok ya da bozuk: varsayılanlar */
    }
  }

  /** Seçilen renk düğmenin rengi olur ve "son kullanılanlar"ın başına geçer */
  use(kind: ColorKind, color: string) {
    this[kind] = color;
    this.recent = [color, ...this.recent.filter((c) => c !== color)].slice(0, 10);
    try {
      localStorage.setItem(KEY, JSON.stringify({ fore: this.fore, back: this.back, shade: this.shade, recent: this.recent }));
    } catch {
      /* yalnızca bu oturumda kalır */
    }
  }
}

export const editorColors = new EditorColors();
