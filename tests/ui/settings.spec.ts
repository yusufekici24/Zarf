// Seçenekler penceresi → Gelişmiş → Veritabanı: boyut, kullanılmayan alan ve sıkıştırma
import { expect, test } from "@playwright/test";
import { calls, fixtures, installMock } from "./mock-backend";

test("veritabanı boyutu gösterilir, sıkıştırma onayla çalışır ve sonucu bildirilir", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await installMock(page, fixtures({ window: "w-settings" }));
  await page.goto("/");
  // Ana pencere ayarlar penceresine içeriğini gönderir
  await page.waitForFunction(() => (window as unknown as { __mock: { listening: (e: string) => boolean } }).__mock.listening("window://assign"));
  await page.evaluate(() =>
    (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("window://assign", { kind: "settings", payload: { tab: "advanced" } }),
  );
  const storage = page.getByTestId("storage");
  await expect(storage).toContainText("Boyut: 1.4 GB (kullanılmayan alan: 200.3 MB)");
  await expect(storage).toContainText("Silinen postaların bıraktığı boş alanı dosyadan geri alır");

  // Vazgeçilirse hiçbir şey yapılmaz
  await storage.getByRole("button", { name: "Veritabanını sıkıştır" }).click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Vazgeç" }).click();
  expect(await calls(page, "compact_database")).toHaveLength(0);

  await storage.getByRole("button", { name: "Veritabanını sıkıştır" }).click();
  await expect(page.getByText("Veritabanı sıkıştırılsın mı?")).toBeVisible();
  await page.getByRole("alertdialog").getByRole("button", { name: "Veritabanını sıkıştır" }).click();
  await expect(page.getByText("Veritabanı sıkıştırıldı: 1.4 GB → 1.2 GB")).toBeVisible();
  await expect(storage).toContainText("kullanılmayan alan: 0 B");
  await expect(storage).toContainText("arka planda kendiliğinden geri alınır");
  expect(await calls(page, "compact_database")).toHaveLength(1);
  expect(errors).toEqual([]);
});
