# Durum

Bir oturumun sonunda güncellenir: son sürümde ne var, neye güvenilmez, sırada ne var. Yeni oturumda önce bu
dosya, sonra yalnızca ilgili konu belgesi okunur (`agent.md` → "Konu belgeleri").

## Son sürüm: 1.7.6 (2026-11-11)

Exchange istek sınırı düzeltmesi (aşağıda). 1.7.5'in (2026-11-10) içeriği:

| Konu | Yapılan | Ayrıntı |
|---|---|---|
| Tablolar | `tfoot` ve tablo öznitelikleri korunur; hücreler harf harf bölünmez; ölçülü resimler oranını korur | `docs/ileti-govdesi.md` |
| Konuşmalar | Bağ, Exchange konuşma kimliği ve `References`/`In-Reply-To` ile kurulur; aynı konulu ilgisiz iletiler birleşmez | `docs/konusmalar.md` |
| Konuşmada okundu | Baş satır içinde okunmamış varken kalın; açılınca bütün konuşma okundu olur | `docs/konusmalar.md` |
| Arama | VEYA / DEĞİL / parantez, yeni alanlar, göreli tarihler; arama kutusunda alan, değer ve kişi önerisi | `docs/arama.md` |

Test durumu: `cargo test --lib` 108 geçti (5 ignore), `npm run check` 0 hata / 15 uyarı, `npm run test:ui` 30 geçti.

## 1.7.6: Exchange istek sınırı (`ErrorServerBusy`)

Bir kullanıcı 1.7.5'te `lynora arka plan: Exchange: The server cannot service this request right now` hatası
aldı: Exchange istek sınırı (`ErrorServerBusy`). Olası tetikleyici, `ews_sync_v3` taramasının bütün klasörleri
art arda sayması ve istemcinin sunucunun istediği bekleme süresine uymaması.

| Ne | Nerede |
|---|---|
| Meşgul hatası türü (`MailError::busy`, `is_busy`) | `src-tauri/src/mail/mod.rs` |
| Bekleme süresinin okunması (`busy_backoff`), sunucu başına kapı (`busy_left`, `set_busy`, `was_busy`), `call` → `call_once` | `src-tauri/src/mail/ews.rs` (test: `server_busy_backoff`) |
| Gecikmeli yeniden sıraya koyma (`defer_after`), seyrek bildirim (15 dk'da bir, yalnızca ön kuyruk) | `src-tauri/src/sync.rs` |
| Taramanın süre dolunca kaldığı sayfadan sürmesi; sınıra takılmış sunucuda sayfalar arası duraklama | `src-tauri/src/ews_sync.rs` `ews_enumerate` |
| Metin `error.ews_busy` | `locales/*.json` (1585 anahtar) |

Kurallar: 8 sn'ye kadar bekleme istek içinde beklenir, daha uzunu hata olarak döner; süre yoksa 30 sn, üst sınır
10 dk. Gerçek sunucuda denenmedi (yalnızca XML örneği).

## İlk açılışta olanlar (1.7.5'e yükseltmede bir kez)

- `subject_index_v2`: konu dizini (`ns`) yeniden yazılır.
- `threads_v1`: bütün iletilerin konuşması kurulur (100 bin iletide birkaç saniye).
- `ews_sync_v3`: Exchange klasörleri bir kez baştan sayılır; konuşma kimlikleri bu taramada dolar. Tarama
  bitene kadar eski Exchange iletileri konu yedek yoluyla bağlı görünür.

## Doğrulanmamış noktalar

- EWS konuşma özellikleri (`0x3013`, `0x0071`, `0x1042`, `0x1039`) yalnızca XML örneğiyle sınandı; gerçek
  Exchange'de ilk eşitlemeden sonra konuşmaların doğru birleştiğine bakılmalı.
- Yazma penceresinde alıntılanan tabloların görünümü (`RichEditor.svelte` CSS'i) gözle denenmedi.
- `ErrorServerBusy` işleyişi gerçek sunucuda görülmedi; etkilenen kullanıcının yeni paketle denemesi gerekir.
- Paket yerelde derlendi; kurulu sistemde yükseltme (1.5.0 → 1.7.5) elle denenmedi.

## Sıradaki işler (istenirse)

- Kalıcı günlük dosyası (`lynora.log`): arka plan ve arayüz hataları şimdi yalnızca stderr'e gidiyor; kullanıcıdan
  hata almak için uygulamayı uçbirimden başlatmak gerekiyor.
- `gizli:` (bcc) araması dizinsiz; çok büyük kutuda yavaş olabilir. Gerekirse `message_fts`'e bcc sütunu.
- Arama kutusunda `klasör:` için gerçek klasör adlarının önerilmesi (şimdilik yalnızca roller).
- IMAP hesaplarında `X-GM-THRID` (Gmail) konuşma kimliği olarak kullanılabilir.
