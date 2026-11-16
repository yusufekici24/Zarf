# Durum

Bir oturumun sonunda güncellenir: son sürümde ne var, neye güvenilmez, sırada ne var. Yeni oturumda önce bu
dosya, sonra yalnızca ilgili konu belgesi okunur (`agent.md` → "Konu belgeleri").

## Zarf 2.0.0 yayın hazırlığı (2026-11-17)

Kırmızı-gri logo/açılış, yerel ve CardDAV kişi düzenleme, Exchange salt okunur/kopya
akışı, yinelenen adres uyarısı, kişi ve zamanlayıcı sayfalaması, seçili takvim hesabıyla
müsaitlik sorgulama, isteğe bağlı EWS süre ölçümü ve açılış Performance API ölçümleri.
Önceki yazma penceresi geliştirmeleri bu sürüme dahildir.
Doğrulama: Rust 108 başarılı / 5 ignore, Svelte 0 hata / 15 mevcut uyarı.
Gerçek Exchange ve GTK kabulü, üretim verisi üzerinde sorgu planı ve p95 ölçümü
henüz yapılmadı. Kayıt başına kalıcı CardDAV eşitleme hatası gösterimi henüz yok.

## Çalışma ağacı: Zarf 2.0.0 (2026-11-17)

- Ürün adı Zarf; vektör logo ve paket simgeleri yenilendi. GTK ve web açılış hareketi
  düzenlendi, hareket azaltma tercihi desteklendi. Eski veri ve anahtarlık kimlikleri korundu.
- Zamanlayıcı grup üyelerini (iç içe gruplar dahil) açar, tekilleştirir, kaynak grubu gösterir.
  `merged` bilgisi ayrıntısız da çizilir; bilinmeyen durum otomatik seçimde boş sayılmaz.
- Kişi listesinde O(n²) kopyalama kaldırıldı, arama beklemesi ve eski yanıt koruması eklendi.
- Kişi ekleme/düzenleme değerlendirmesi ve ölçümlü performans raporu: `docs/zarf-2.0.md`.
- Doğrulama: Rust 108 geçti / 5 ignore; UI 42 geçti (2 worker); svelte-check 0 hata / 15
  mevcut uyarı. İlk 8 worker UI koşusunda 5 açılış zaman aşımı oldu, 2 worker ile tamamı geçti.
- Paket üretildi: `src-tauri/target/release/bundle/deb/Zarf_2.0.0_amd64.deb`.
  Paket kimliği zarf 2.0.0; Lynora ve MailNix için Conflicts/Replaces/Provides doğrulandı.
  APT simülasyonu kurulu lynora 1.7.6 yerine zarf 2.0.0 kurulumunu çözdü; gerçek kurulum yapılmadı.
- Gerçek Exchange, GTK ekranında görsel kabul ve kurulu sistemde yükseltme henüz denenmedi.
- Önceden çalışma ağacında bulunan yazma penceresi değişiklikleri korundu ve testlere dahildir.

## Son sürüm: 1.7.6 (2026-11-11)

Exchange istek sınırı düzeltmesi (aşağıda). 1.7.5'in (2026-11-10) içeriği:

| Konu | Yapılan | Ayrıntı |
|---|---|---|
| Tablolar | `tfoot` ve tablo öznitelikleri korunur; hücreler harf harf bölünmez; ölçülü resimler oranını korur | `docs/ileti-govdesi.md` |
| Konuşmalar | Bağ, Exchange konuşma kimliği ve `References`/`In-Reply-To` ile kurulur; aynı konulu ilgisiz iletiler birleşmez | `docs/konusmalar.md` |
| Konuşmada okundu | Baş satır içinde okunmamış varken kalın; açılınca bütün konuşma okundu olur | `docs/konusmalar.md` |
| Arama | VEYA / DEĞİL / parantez, yeni alanlar, göreli tarihler; arama kutusunda alan, değer ve kişi önerisi | `docs/arama.md` |

Test durumu: `cargo test --lib` 108 geçti (5 ignore), `npm run check` 0 hata / 15 uyarı, `npm run test:ui` 30 geçti.

## Sürümlenmemiş: yazma penceresi (2026-11-17, çalışma ağacında, commit yok)

Hata: yazma alanında vurgu rengi ilk satıra uygulanıyor, ikinci satır seçilip aynı renk seçilince uygulanmıyordu.
Neden: düğme yerel `<input type="color">`'ın `change` olayına bağlıydı; o olay yalnızca değer değişince gelir.
Düzeltmeyle birlikte yazma penceresi genişletildi (ayrıntı: `docs/yazma-penceresi.md`):

| Ne | Nerede |
|---|---|
| Bölünmüş renk düğmesi + palet (tema/standart/son kullanılan/özel renk) | `components/editor/ColorButton.svelte`, `editor-colors.svelte.ts` |
| Ortak biçim araçları, açık biçimin vurgulanması, alt/üst simge, iki yana yasla, büyüt/küçült, satır aralığı, harf dönüşümü | `components/editor/EditorTools.svelte`, `RichEditor.svelte` |
| Seçim araç çubuğu, biçim boyacısı, bağlantı kartı (Ctrl+K), Sekme davranışı | `RichEditor.svelte` |
| Paragraf stilleri galerisi (gönderirken satır içi stil) | `editor.ts`, `ComposeWindow.outgoingHtml` |
| Bağlamsal "Tablo" sekmesi: stiller, satır/sütun, gölgelendirme, genişlik | `editor-table.ts`, `ComposeWindow.svelte` |
| Ek kartları, durum çubuğu (sözcük sayısı, taslak durumu, yakınlaştırma), Tarih ve Saat | `ComposeWindow.svelte` |
| Yan düzeltmeler: imleç boşken seçilen punto dev yazıyordu; tablo ızgarası menüsü seçince kapanmıyordu; eklenen tablonun kenarlık türü satır içinden düşüyordu | `RichEditor.svelte`, `ComposeWindow.svelte` |

Test: `npm run check` 0 hata / 15 uyarı, `npm run test:ui` 40 geçti (10'u `compose.spec.ts`). Metinler
`locales/*.json` (1649 anahtar). Sürüm numarası değişmedi (1.7.6); paket derlenmedi.

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
- Yeni yazma penceresi araçları yalnızca Chrome'da (Playwright) sınandı; uygulamanın kendi motorunda (WebKitGTK)
  gözle denenmeli: renk düğmesi ve palet, seçim araç çubuğu, biçim boyacısı, stiller, tablo sekmesi, yakınlaştırma.
- `ErrorServerBusy` işleyişi gerçek sunucuda görülmedi; etkilenen kullanıcının yeni paketle denemesi gerekir.
- Paket yerelde derlendi; kurulu sistemde yükseltme (1.5.0 → 1.7.5) elle denenmedi.

## Sıradaki işler (istenirse)

- Kalıcı günlük dosyası (`lynora.log`): arka plan ve arayüz hataları şimdi yalnızca stderr'e gidiyor; kullanıcıdan
  hata almak için uygulamayı uçbirimden başlatmak gerekiyor.
- `gizli:` (bcc) araması dizinsiz; çok büyük kutuda yavaş olabilir. Gerekirse `message_fts`'e bcc sütunu.
- Arama kutusunda `klasör:` için gerçek klasör adlarının önerilmesi (şimdilik yalnızca roller).
- IMAP hesaplarında `X-GM-THRID` (Gmail) konuşma kimliği olarak kullanılabilir.
