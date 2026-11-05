// Arayüz testleri: ön yüz Vite geliştirme sunucusunda, sahte arka uçla (tests/ui/mock-backend.ts)
// sistemdeki Chrome'da çalışır (tarayıcı indirilmez). Çalıştırma: npm run test:ui
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests/ui",
  timeout: 30_000,
  fullyParallel: true,
  reporter: [["list"]],
  use: {
    channel: "chrome",
    baseURL: "http://127.0.0.1:1420",
    viewport: { width: 1400, height: 900 },
    locale: "tr-TR",
    timezoneId: "Europe/Istanbul",
  },
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
