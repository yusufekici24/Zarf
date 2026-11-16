import { expect, test } from "@playwright/test";
import { fixtures, installMock } from "./mock-backend";

test("kişi listesi sayfalanır; Exchange çift tıklaması salt okunur, kişisel kopya düzenlenebilir", async ({ page }) => {
  await installMock(page, fixtures({ settings: { language: "tr", start_module: "contacts" } }));
  await page.addInitScript(() => {
    const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a?: unknown) => Promise<unknown> } };
    const original = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === "list_groups") return [];
      if (cmd === "list_contacts") return Array.from({ length: 150 }, (_, i) => ({ id: i + 1, display_name: `Kişi ${i}`, first_name: "Kişi", last_name: String(i), email: `k${i}@test.local`, email2: "", phone_mobile: "", phone_work: "", phone_home: "", company: "", job_title: "", department: "", address: "", website: "", birthday: "", notes: "", categories: "", is_favorite: false, account_id: i === 0 ? 1 : null, remote_id: i === 0 ? "remote" : null }));
      return original(cmd, args);
    };
  });
  await page.goto("/");
  await expect(page.getByText("1 / 2 · 150")).toBeVisible();
  await expect(page.getByText("Kişi 149", { exact: true })).toHaveCount(0);
  await page.getByText("Kişi 0", { exact: true }).dblclick();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.getByRole("button", { name: "Kişisel kopya oluştur" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByRole("button", { name: "Vazgeç", exact: true }).click();
  await page.getByRole("button", { name: "›", exact: true }).click();
  await expect(page.getByText("Kişi 149", { exact: true })).toBeVisible();
});
