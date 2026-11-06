// Renk teması (Outlook "Office teması" rengi): üst çubuktaki palet ya da Seçenekler > Genel > Renk.
// Vurgu rengi --primary'dir; seçili satır, üzerine gelme ve hafif vurgu tonları ondan türetilir
// (--primary-soft / --primary-hover / --primary-faint). Açık ve koyu görünüm renkleri app.css'te
// html[data-accent] kurallarıyla tanımlıdır; burada adları ve örnek (açık görünüm) renkleri var.

export const ACCENTS = [
  { key: "blue", label: "accent.blue", color: "#0f6cbd" },
  { key: "purple", label: "accent.purple", color: "#6b46b8" },
  { key: "red", label: "accent.red", color: "#c50f1f" },
  { key: "pink", label: "accent.pink", color: "#bf0077" },
  { key: "orange", label: "accent.orange", color: "#b54708" },
  { key: "green", label: "accent.green", color: "#107c41" },
  { key: "teal", label: "accent.teal", color: "#00727a" },
  { key: "graphite", label: "accent.graphite", color: "#525e6b" },
] as const;

export const ACCENT_DEFAULT = "blue";

export function applyAccent(key: string) {
  const el = document.documentElement;
  if (key !== ACCENT_DEFAULT && ACCENTS.some((a) => a.key === key)) el.dataset.accent = key;
  else delete el.dataset.accent;
}
