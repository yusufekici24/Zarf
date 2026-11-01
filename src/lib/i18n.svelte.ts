// Reaktif çeviri sistemi. Çeviriler Rust tarafından `locales/*.json` dosyalarından
// yüklenir; böylece yeni bir dil eklemek için yalnızca bir JSON dosyası eklemek yeterlidir.
import { api } from "./api";

class I18n {
  current = $state("tr");
  langs = $state<Record<string, Record<string, string>>>({});

  async load() {
    const r = await api.getLocales();
    this.langs = r.langs;
    this.current = r.current;
    document.documentElement.lang = this.current;
  }

  async set(code: string) {
    this.current = await api.setLanguage(code);
    document.documentElement.lang = this.current;
  }

  get available(): { code: string; name: string }[] {
    return Object.entries(this.langs)
      .map(([code, m]) => ({ code, name: m["@name"] ?? code }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  /** BCP 47 yerel ayarı (tarih/sayı biçimleri için) */
  get locale(): string {
    return this.langs[this.current]?.["@locale"] ?? this.current;
  }

  t = (key: string, ...args: (string | number)[]): string => {
    let s = this.langs[this.current]?.[key] ?? this.langs["en"]?.[key] ?? key;
    args.forEach((a, i) => (s = s.replaceAll(`{${i}}`, String(a))));
    return s;
  };
}

export const i18n = new I18n();
export const t = i18n.t;
