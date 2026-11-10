// Arama kutusu: arama alanı önerileri (kimden:, durum: …), alan değerleri, alan içinde kişi
// önerisi ve gelişmiş arama formunun sorguyu alanlara ayırması
import { expect, test } from "@playwright/test";
import { calls, fixtures, installMock } from "./mock-backend";

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { errors: string[] }).errors = errors;
  await installMock(page, fixtures());
  await page.goto("/");
  await expect(page.getByText("Aylık rapor").first()).toBeVisible();
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { errors: string[] }).errors, "sayfada yakalanmamış hata olmamalı").toEqual([]);
});

const searched = async (page: import("@playwright/test").Page) => (await calls(page, "list_messages")).map((a) => String(a.search ?? "").trim());

test("alan önerisi: 'kim' yazınca 'kimden:' önerilir, seçilince kutuya yazılır ve değer beklenir", async ({ page }) => {
  const box = page.getByPlaceholder(/ara/i).first();
  await box.click();
  await box.pressSequentially("kim");
  const list = page.getByTestId("search-suggestions");
  await expect(list.getByText("Arama alanları")).toBeVisible();
  await list.getByRole("option", { name: /kimden:/ }).click();
  await expect(box).toHaveValue("kimden:");
  await expect(box).toBeFocused();
  // Alan içinde kişi önerilir; seçilen kişinin adresi alana yazılır ve aranır
  await box.pressSequentially("ay");
  await list.getByRole("option", { name: /Ayşe Yılmaz/ }).click();
  await expect(box).toHaveValue("kimden:ayse.yilmaz@kurum.com.tr ");
  await expect.poll(() => searched(page)).toContain("kimden:ayse.yilmaz@kurum.com.tr");
});

test("değer önerisi: 'durum:ok' → okunmamış; Sekme tamamlar ve arar", async ({ page }) => {
  const box = page.getByPlaceholder(/ara/i).first();
  await box.click();
  await box.pressSequentially("durum:ok");
  await expect(page.getByTestId("search-suggestions").getByRole("option", { name: "okunmamış" })).toBeVisible();
  await box.press("Tab");
  await expect(box).toHaveValue("durum:okunmamış ");
  await expect.poll(() => searched(page)).toContain("durum:okunmamış");
});

test("alan yazılırken Enter öneriyi değil yazılanı arar", async ({ page }) => {
  const box = page.getByPlaceholder(/ara/i).first();
  await box.click();
  await box.pressSequentially("kimden:ay");
  await expect(page.getByTestId("search-suggestions").getByRole("option", { name: /Ayşe Yılmaz/ })).toBeVisible();
  await box.press("Enter");
  await expect(box).toHaveValue("kimden:ay");
  await expect.poll(() => searched(page)).toContain("kimden:ay");
});

test("gelişmiş arama: sorgu alanlara ayrılır ve sözdizimi ipucu görünür", async ({ page }) => {
  const box = page.getByPlaceholder(/ara/i).first();
  await box.click();
  await box.fill('Kimden: ayşe konu:"aylık rapor" bütçe');
  await box.press("Enter");
  await page.getByTitle("Gelişmiş arama").click();
  await expect(page.getByTestId("search-hint")).toBeVisible();
  await expect(page.locator("#adv-from")).toHaveValue("ayşe");
  await expect(page.locator("#adv-subj")).toHaveValue("aylık rapor");
  await expect(page.locator("#adv-words")).toHaveValue("bütçe");
});

test("sözdizimi çözümleyici: parçalar, boşluklu anahtar, olumsuz ve öneriler", async ({ page }) => {
  const r = await page.evaluate(async (path) => {
    const s: typeof import("../../src/lib/search-syntax") = await import(/* @vite-ignore */ path);
    const p = s.parts('Kimden: ali -konu:"yıllık izin" rapor from:x@y.z');
    return {
      keys: p.map((x) => (x.neg ? "-" : "") + (x.op?.id ?? "") + "=" + x.value),
      ops: s.hints("gön", "tr").map((h) => h.label),
      en: s.hints("is:un", "en").map((h) => h.insert),
      neg: s.hints("rapor -ek:v", "tr").map((h) => h.insert),
      none: s.hints("ayse@kurum.com", "tr").length,
      repl: s.replaceCurrent("rapor kim", "kimden:"),
    };
  }, "/src/lib/search-syntax.ts");
  expect(r.keys).toEqual(["from=ali", "-subject=yıllık izin", "=rapor", "from=x@y.z"]);
  expect(r.ops).toEqual(["kimden:"]);
  expect(r.en).toEqual(["is:unread ", "is:unreplied "]);
  expect(r.neg).toEqual(["-ek:var "]);
  expect(r.none).toBe(0);
  expect(r.repl).toBe("rapor kimden:");
});
