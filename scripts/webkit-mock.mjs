#!/usr/bin/env node
// Arayüz testlerinin sahte arka ucunu (tests/ui/mock-backend.ts) düz bir başlangıç betiğine çevirir;
// scripts/webkit-dene.py bu betiği gerçek WebKitGTK'ye yükler.
//   node scripts/webkit-mock.mjs cikti.js ['{"theme":"light","me_highlight":"tocc"}'] ['{"window":"w-compose"}']
// Üçüncü bağımsız değişken fixtures() üzerine yazılır (ör. pencere etiketi; yazma penceresine içerik
// sayfada `__mock.emit("window://assign", …)` ile verilir, bkz. tests/ui/compose.spec.ts).
// İletiler: yarısı okunmamış, Kime/Bilgi satırları çeşitli (hesap: ben@kurum.com.tr).
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const mod = await import(fileURLToPath(new URL("../tests/ui/mock-backend.ts", import.meta.url)));
const out = process.argv[2];
if (!out) throw new Error("kullanım: webkit-mock.mjs cikti.js [ayarlar-json]");
const settings = JSON.parse(process.argv[3] || "{}");

const now = Math.floor(Date.now() / 1000);
const TO = ["Ben <ben@kurum.com.tr>", "Tüm Personel <tum@kurum.com.tr>", "Ekip <ekip@kurum.com.tr>", "Ali Veli, Deneme KULLANICI"];
const CC = ["", "", "ben@kurum.com.tr", ""];
const KIND = ["Kime: ben", "herkese", "Bilgi: ben", "yalnız ad"];
const messages = Array.from({ length: 8 }, (_, k) => ({
  id: k + 1,
  folder_id: 1,
  subject: `Deneme iletisi ${k + 1} (${KIND[k % 4]})`,
  sender: `Ayşe Yılmaz ${k + 1} <ayse${k + 1}@kurum.com.tr>`,
  date: now - (k + 1) * 900,
  snippet: "Yarınki bütçe toplantısı saat 10:00'da",
  is_read: k >= 4,
  to: TO[k % 4],
  cc: CC[k % 4],
  html: "<p>Merhaba</p>",
}));

const over = JSON.parse(process.argv[4] || "{}");
const f = mod.fixtures({ messages, ...over, settings: { language: "tr", start_module: "mail", reading_pane: "right", sync_on_start: "0", mark_read_delay: "-1", ...settings } });
let script = "";
await mod.installMock({ addInitScript: async (fn, arg) => void (script = `(${fn.toString()})(${JSON.stringify(arg)});`) }, f);
writeFileSync(out, script);
