// Güvenilir göndericiler: adres/alan adı kuralları, okuma bölmesinden alan adına güvenme ve Güven Merkezi'nden ekleme
import { expect, test, type Page } from "@playwright/test";
import { calls, fixtures, installMock, type MockMessage } from "./mock-backend";

const NOW = Math.floor(Date.UTC(2026, 10, 5, 9, 0) / 1000);
const remote = (id: number, subject: string, sender: string): MockMessage => ({
  id, folder_id: 1, subject, sender, date: NOW - id * 60, snippet: subject, is_read: true, has_remote: true, html: `<p>${subject}</p>`,
});
const MESSAGES = [
  remote(1, "Haftalık bülten", "Ayşe Yılmaz <ayse.yilmaz@kurum.com.tr>"),
  remote(2, "Toplantı notları", "Mehmet Kaya <Mehmet.Kaya@Kurum.com.tr>"),
  remote(3, "Kampanya", "Dükkan <kampanya@gmail.com>"),
];
const BLOCKED = "Gizliliğinizi korumak için bu iletideki uzak resimler indirilmedi.";

function trackErrors(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  return errors;
}

async function trustedSaves(page: Page) {
  return (await calls(page, "set_setting")).filter((a) => a.key === "trusted_senders").map((a) => JSON.parse(String(a.value)));
}

test("kurallar: alan adı biçimleri, alt alan adları ve benzer adlı alanlar", async ({ page }) => {
  await installMock(page, fixtures());
  await page.goto("/");
  // Kurallar sayfanın içinde, Vite'ın sunduğu modülle sınanır (uygulamanın kullandığı kodun aynısı)
  const r = await page.evaluate(async (path) => {
    const m: typeof import("../../src/lib/trust") = await import(/* @vite-ignore */ path);
    const n = m.normalizeTrustEntry;
    return {
      norm: ["sirket.com.tr", "@SIRKET.com.tr", "*.sirket.com.tr", "*@sirket.com.tr", "Ali Veli <Ali.Veli@SIRKET.com.tr>", "sirket", "ali veli@sirket.com.tr", "ali@", ""].map(n),
      match: [
        m.matchesTrusted("ali@sirket.com.tr", ["@sirket.com.tr"]),
        m.matchesTrusted("ali@posta.sirket.com.tr", ["@sirket.com.tr"]),
        m.matchesTrusted("Ali@SIRKET.com.tr", ["sirket.com.tr"]), // eski sürümde kaydedilmiş biçim de geçerli
        m.matchesTrusted("ali@kotusirket.com.tr", ["@sirket.com.tr"]),
        m.matchesTrusted("ali@sirket.com.tr.saldirgan.com", ["@sirket.com.tr"]),
        m.matchesTrusted("ali@sirket.com.tr", ["ali@sirket.com.tr"]),
        m.matchesTrusted("veli@sirket.com.tr", ["ali@sirket.com.tr"]),
        m.matchesTrusted("", ["@sirket.com.tr"]),
      ],
      domain: ["ali@sirket.com.tr", "Ali@Posta.SIRKET.com.tr", "dukkan@gmail.com", "x@yandex.com.tr", ""].map(m.trustDomainOf),
    };
  }, "/src/lib/trust.ts");
  expect(r.norm).toEqual(["@sirket.com.tr", "@sirket.com.tr", "@sirket.com.tr", "@sirket.com.tr", "ali.veli@sirket.com.tr", null, null, null, null]);
  expect(r.match).toEqual([true, true, true, false, false, true, false, false]);
  expect(r.domain).toEqual(["@sirket.com.tr", "@posta.sirket.com.tr", null, null, null]);
});

test("okuma bölmesi: alan adına güvenince o kurumdan gelen hiçbir ileti yeniden sormaz", async ({ page }) => {
  const errors = trackErrors(page);
  await installMock(page, fixtures({ messages: MESSAGES }));
  await page.goto("/");

  await page.getByText("Haftalık bülten").first().click();
  await expect(page.getByText(BLOCKED)).toBeVisible();
  await expect(page.getByRole("button", { name: "Bu göndericiye güven" })).toBeVisible();
  await page.getByRole("button", { name: "@kurum.com.tr alan adına güven" }).click();
  await expect(page.getByText("@kurum.com.tr güvenilir göndericilere eklendi")).toBeVisible();
  await expect(page.getByText(BLOCKED)).toHaveCount(0);
  expect(await trustedSaves(page)).toEqual([["@kurum.com.tr"]]);

  // Aynı kurumdan başka biri (adres büyük harfli) sorulmadan açılır
  await page.getByText("Toplantı notları").first().click();
  await expect(page.frameLocator('iframe[title="message"]').first().locator("body")).toContainText("Toplantı notları");
  await expect(page.getByText(BLOCKED)).toHaveCount(0);

  // Herkese açık posta servisinde alan adına güvenme önerilmez
  await page.getByText("Kampanya").first().click();
  await expect(page.getByText(BLOCKED)).toBeVisible();
  await expect(page.getByRole("button", { name: "Bu göndericiye güven" })).toBeVisible();
  await expect(page.getByRole("button", { name: /alan adına güven/ })).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("Güven Merkezi: 'sirket.com.tr' yazılınca alan adı eklenir, geçersiz giriş bildirilir", async ({ page }) => {
  const errors = trackErrors(page);
  await installMock(page, fixtures({ window: "w-settings" }));
  await page.goto("/");
  await page.waitForFunction(() => (window as unknown as { __mock: { listening: (e: string) => boolean } }).__mock.listening("window://assign"));
  await page.evaluate(() =>
    (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("window://assign", { kind: "settings", payload: { tab: "trust" } }),
  );

  const add = async (value: string) => {
    await page.getByRole("button", { name: "Gönderici ekle" }).click();
    await page.getByLabel("Adres ya da alan adı").fill(value);
    await page.getByRole("dialog").getByRole("button", { name: "Tamam" }).click();
  };

  await expect(page.getByText("Güvenilir gönderici yok.")).toBeVisible();
  await add("sirket.com.tr");
  await expect(page.getByText("@sirket.com.tr", { exact: true })).toBeVisible();
  expect(await trustedSaves(page)).toEqual([["@sirket.com.tr"]]);

  // Aynı alan adı farklı yazılınca ikinci kez eklenmez
  await add("*.SIRKET.com.tr");
  await expect(page.getByText("@sirket.com.tr", { exact: true })).toHaveCount(1);

  await add("sirket");
  await expect(page.getByText('"sirket" geçerli bir adres ya da alan adı değil.')).toBeVisible();
  expect(await trustedSaves(page)).toEqual([["@sirket.com.tr"]]);
  expect(errors).toEqual([]);
});
