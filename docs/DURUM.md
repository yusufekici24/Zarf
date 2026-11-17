# Durum

Bir oturumun sonunda güncellenir: son sürümde ne var, neye güvenilmez, sırada ne var. Yeni oturumda önce bu
dosya, sonra yalnızca ilgili konu belgesi okunur (`agent.md` → "Konu belgeleri").

## Zarf 2.0.4 (2026-11-17)

Kullanıcının 2.0.3 `perf.log`'u (aynı model makine, ekran ölçeği 2): ana pencere 1,4 sn'de görünür, 0,3–0,5 sn'de
tıklanabilir; okuma bölmesinde ileti açılışları 300 ms altında (satır yok), yavaş Exchange çağrısı yok. Açılışın
~3,35. saniyesinde ana pencerede tıklamalar 184–427 ms geç işleniyordu, sayfa kilitli değildi.

| Bulgu | Düzeltme | Nerede |
|---|---|---|
| Yeni yedek pencere kurmak GTK ana döngüsünü ~250 ms donduruyor (yeni `gtk_kilit` ölçümüyle, gerçek uygulamada 3/3); kurulum sabit zamanlarda oluyordu: açılışın 3. saniyesi ve her pencere açılışından 0,8 sn sonra, yani tam kullanıcı tıklarken | Yedek, kullanıcı son 2 sn'de hiçbir pencerede tıklamadıysa/yazmadıysa kurulur; pencere açma isteği ve ilk çizim de etkinlik sayılır. Pencere yedekten ~180 ms'de açılıyor, yeni yedek ~2,3 sn sonra boşta | `windows.rs` `spare_scheduler`, `spare_due`, `app_activity`; `src/lib/activity.ts` |
| 2.0.3 log'undaki `ui[w-1]`/`ui[w-2]` 3 sn'lik "kilit" satırları gerçek değil: gizli yedek pencerede WebKit tekrarlanan zamanlayıcıları kısıyor, pencere gösterilince gelen geç tik kilit sayılıyordu (görünürlük olayı tikten sonra geliyor) | Önceki tik gizliyken geldiyse boşluk sayılmaz | `src/lib/perf.ts` |
| Ölçüm eksikleri | `gtk_kilit` (GTK döngüsü ≥150 ms gecikme), `pencere_açıldı` (açma isteğinden ilk kareye, yedekten mi), `yedek_pencere_kuruluyor/kuruldu` | `perf.rs` `watch_main_loop`, `windows.rs` |

Deneme yöntemi (sudo ve gerçek veri gerekmez): geçici veri klasörüyle (`LYNORA_DATA_DIR`, ayarlarda
`welcome_shown=1` ki otomatik hesap kurulumu kurum sunucusunu yoklamasın) `target/release/lynora` başlatılır, ikinci
çağrı `lynora dosya.eml` yedek pencereyi kullanır; sonuç o klasördeki `perf.log`'da. Not: düz `cargo build --release`
arayüzü gömmez (geliştirme sunucusunu arar), `npx tauri build --no-bundle` kullanın.

Test: `cargo test --lib` 111 geçti / 6 ignore; `npm run check` 0 hata / 15 uyarı; `npm run test:ui` 63 geçti.

## Zarf 2.0.3 (2026-11-17)

| Ne | Nerede |
|---|---|
| Açılıştan kısa süre sonra ekranın "yakınlaşıp yazı tipinin değişmesi": pencere, kullanılan bütün Noto Sans yüzleri (400/500/600/700, italik 400/700; Türkçe harf kümesi dahil) yüklenince gösterilir | `uifont.ts` `uiFontsReady`, `+layout.svelte`, `+page.svelte` (revealWindow) |
| Yeni iletiye tıklayınca yavaş açılma: eşitlemede yeni ileti geldiyse en yeni 5 iletinin gövdesi hemen ön kuyrukta iner (önceden arka plan kuyruğunun sonunda, taramalar/takvim/kişiler bitene kadar bekliyordu) | `ews_sync.rs` `ews_prefetch_new`, `ews_sync_folder` |
| Gövde 120 ms'de gelmezse okuma bölmesinde başlık + önizleme + "İleti indiriliyor…" (önceki ileti ekranda kalıyordu) | `mail.svelte.ts` `show`, `placeholderView`; `ReadingPane.svelte` `loadingBody` |
| Kalıcı performans günlüğü `~/.local/share/lynora/perf.log` (her zaman açık, yalnızca yavaş olaylar, kişisel içerik yok): sistem bilgisi, ana pencerenin gösterilme anı, açılışta "tıklanabilir" an, arayüz kilitlenmeleri (≥300 ms), geç işlenen tıklamalar, ileti açılışı (indirme/ayrıştırma/çizim), yavaş/meşgul Exchange çağrıları (≥1,5 sn) | `src-tauri/src/perf.rs`, `commands::perf_log`, `src/lib/perf.ts` |

Yazı tipi bulgusu (gizli WebKitGTK penceresinde, `WEBKIT_DENE_GIZLI=1`): düzeltmesiz sürümde pencere gösterildiği anda
12 Noto Sans yüzünden 5'i yüklüydü, kalanlar sonradan geliyordu; düzeltmeyle 12'si de gösterilmeden önce yüklü.
Kullanıcının makinesindeki etkinin tamamen bu olduğu doğrulanmadı (görsel deneme yapılamadı). Açılıştan 6 sn sonra
`watchScreen` ekran boyutunu farklı okursa ölçek yeniden uygulanır; sorun sürerse ikinci aday budur.

Yeni ileti gövdesi: tıklama hâlâ indirmeyi beklerse (ileti ön yükleme bitmeden açıldıysa) süreyi bağlantı kurulumu
(Kerberos + TLS, havuzda bağlantı yoksa), ekli büyük MIME ya da Exchange istek sınırı belirler; `ZARF_PERF=1`
çıktısındaki `message_content_wait` bunu gösterir.

Bildirilen: açılışta çizim sonrası 2–3 sn tıklanamıyor, yeni ileti 2–3 sn'de açılıyor, sonra tıklamalar 1–2 sn.
Bu makinede (Core Ultra 7) derlenmiş arayüz WebKitGTK'de: açılışta tek kilit 171 ms (pencere gösterilmeden önce),
sonraki 8 sn'de 60 ms'yi aşan kilit yok; 2 MB resimli 300 satır tablolu ileti açılışında kilit yok. Yavaşlık o
makineye özgü (işlemci, çizim/GPU, ölçek ya da Exchange); karar `perf.log`'a göre verilecek.
`perf.log` okuma: `kilit`/`tıklama_çizim` yüksek ve `ileti_aç indirme_ms` düşükse sorun çizim/işlemci (donanım
hızlandırma, ölçek, yazı tipi denenir); `indirme_ms` ya da `ews süre_ms` yüksekse ağ/Exchange.

Test: `cargo test --lib` 110 geçti / 6 ignore; `npm run check` 0 hata / 15 uyarı; `npm run test:ui` 63 geçti
(4'ü `startup-open.spec.ts`).

## Zarf 2.0.2 (2026-11-17)

| Ne | Nerede |
|---|---|
| Gönderimi geri alma: varsayılan 10 sn (kapalı/5/10/20/30, Seçenekler > Posta > Gönderme); ileti süre dolunca tam zamanında gider (önceden +30 sn pay ve 30 sn'lik denetim yüzünden 30–60 sn gecikebiliyordu); bildirimde geri sayım, "Geri al", "Şimdi gönder" | `undo-send.ts`, `ComposeWindow.send`, `+page.svelte` `undoSendToast`, `sync.rs` `send_later` |
| Gönderim ile geri alma aynı anda kazanamaz (koşullu `pending`: `sending:` / `hold`); gönderilemeyen ileti yeniden denenir; kapanışta yarım kalan gönderim açılışta geri alınır | `db.rs` `swap_pending`, `reset_stuck_sending`; `sync.rs` `send_outbox_item`; `commands.rs` `cancel_outbox` |
| Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z: düzenleyicide kendi geçmişi (satır aralığı, harf dönüşümü, tablo da geri alınır), metin kutularında `execCommand` | `editor-history.ts`, `RichEditor.svelte`, `+layout.svelte`; `docs/yazma-penceresi.md` |
| İki sözcüklü arama hızlandı (150 bin iletide 2,1 sn → 0,6 sn; FTS5 `NEAR`) | `db.rs` `fts_query`; `docs/PERFORMANS.md` |
| Performans ölçümü (gerçek kod, sentetik büyük kutu) ve WebKitGTK'de tuş gönderme | `db.rs` `perf_tests`, `scripts/webkit-dene.py` |

Neden: WebKitGTK 2.52'de Ctrl+Z ve Ctrl+Y ne metin kutusunda ne düzenlenebilir alanda bir şey yapmıyor (betikle
doğrulandı); düzeltme aynı betikle yazma penceresinde ve Konu alanında denendi.

Test: `cargo test --lib` 109 geçti / 6 ignore; `npm run check` 0 hata / 15 uyarı; `npm run test:ui` 59 geçti
(9'u `undo.spec.ts`); metinler 1666 anahtar.

Doğrulanmadı: gerçek Exchange'de gönderimi geri alma (yalnızca birim testi ve arayüz taklidi); Ubuntu 24.04'te
kısayollar (26.04 / WebKitGTK 2.52'de denendi); kullanıcının bildirdiği yavaşlığın kaynağı (yukarıdaki ölçüm
sentetik; o makinede `ZARF_PERF=1 lynora` çıktısı gerekiyor, bkz. `docs/PERFORMANS.md`).

## Önceki sürüm: Zarf 2.0.1 (2026-11-17)

| Ne | Nerede |
|---|---|
| Adı Kime/Bilgi satırında geçen iletiler renkli yazılır (varsayılan kapalı; Seçenekler > Posta > Okuma Bölmesi: kip, renk, ek adlar) | `src/lib/me.ts`, `MessageList.svelte` `toMe`, `SettingsDialog.svelte`, `db.rs` `LIST_COLS` + `MessageRow.cc` |
| İleti sürüklenirken bırakılacak klasör vurgulanır, adı sürükleme etiketinde yazar; kapalı klasör üzerinde beklenince açılır, bölme kenarında liste kayar | `mail.drag.over` (`mail.svelte.ts`), `MessageList.svelte` `folderAt`, `FolderPane.svelte`, `app.css` `.drop-target` |
| Gömülü arayüz yazı tipi değişken Noto Sans yerine kalınlık başına ayrı dosya (400/500/600/700, italik 400/700) | `+layout.svelte`, `uifont.ts`, `app.css`; paket `@fontsource/noto-sans` |
| Uygulama menüsünde eski adlarla da bulunur (`lynora`, `mailnix`, `linux posta`, `outlook`) | `packaging/lynora.desktop` `Keywords` |
| Gerçek WebKitGTK'de deneme betikleri | `scripts/webkit-mock.mjs`, `scripts/webkit-dene.py` |

Sürükleme hatasının nedeni: WebKitGTK fare basılıyken `:hover`'ı güncellemiyor, vurgu `hover:ring` sınıfına bağlıydı
(WebKitGTK 2.52.6'da betikle doğrulandı; düzeltme aynı betikle açık ve koyu görünümde denendi).

Test: `cargo test --lib` 108 geçti / 5 ignore; `npm run check` 0 hata / 15 uyarı; `npm run test:ui` 50 geçti
(6'sı `highlight.spec.ts`); metinler 1663 anahtar.

**Doğrulanmadı: Ubuntu 24.04'te okunmamış satırın üzerine gelince kalınlığını yitirmesi (kullanıcı bildirimi).**
Ubuntu 26.04 / WebKitGTK 2.52.6'da hata yinelenemedi (yazılımla çizimde üzerine gelme öncesi ve sonrası aynı;
hesaplanan stil 700/600 kalıyor). Yazı tipi değişikliği şu tahmine dayanır: eski WebKitGTK değişken yazı tipinin
kalınlık eksenini kısmi yeniden çizimde uygulamıyor (`uifont.ts`'teki Segoe UI Variable sorunuyla aynı sınıf).
2.0.1 ile sorun sürerse o makineden istenecekler: `dpkg -l libwebkit2gtk-4.1-0` sürümü; rengin de değişip
değişmediği; Seçenekler > Gelişmiş > "Donanım hızlandırma" kapatılınca (yazılımla çizim) sorunun kaybolup kaybolmadığı.

Exchange'de adı yalnızca görünen adla eşleştirmek yerine `PR_MESSAGE_TO_ME`/`PR_MESSAGE_CC_ME` (0x0057/0x0058)
özellikleri liste isteğine eklenebilir; sütun ve yeniden tarama gerektirdiği için yapılmadı. Kırmızı vurgu gerçek
Exchange verisiyle denenmedi.

## Önceki sürüm: Zarf 2.0.0 (2026-11-17)

Kırmızı-gri logo/açılış, yerel ve CardDAV kişi düzenleme, Exchange salt okunur/kopya
akışı, yinelenen adres uyarısı, kişi ve zamanlayıcı sayfalaması, seçili takvim hesabıyla
müsaitlik sorgulama, isteğe bağlı EWS süre ölçümü ve açılış Performance API ölçümleri.
Önceki yazma penceresi geliştirmeleri bu sürüme dahildir.
Doğrulama: Rust 108 başarılı / 5 ignore, UI 44 başarılı, Svelte 0 hata / 15 mevcut uyarı.
Klasör sayaçları artık ileti yenilemesini bekletmez; eşzamanlı sayaç çağrıları birleştirilir.
Ayrıntılı ölçümler ve salt okunur tanı betikleri: `docs/PERFORMANS.md`, `scripts/`.
Gerçek Exchange ve GTK kabulü, üretim verisi üzerinde sorgu planı ve p95 ölçümü
henüz yapılmadı. Kayıt başına kalıcı CardDAV eşitleme hatası gösterimi henüz yok.

## Zarf 2.0.0 içeriği (2026-11-17)

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

## Önceki sürüm: 1.7.6 (2026-11-11)

Exchange istek sınırı düzeltmesi (aşağıda). 1.7.5'in (2026-11-10) içeriği:

| Konu | Yapılan | Ayrıntı |
|---|---|---|
| Tablolar | `tfoot` ve tablo öznitelikleri korunur; hücreler harf harf bölünmez; ölçülü resimler oranını korur | `docs/ileti-govdesi.md` |
| Konuşmalar | Bağ, Exchange konuşma kimliği ve `References`/`In-Reply-To` ile kurulur; aynı konulu ilgisiz iletiler birleşmez | `docs/konusmalar.md` |
| Konuşmada okundu | Baş satır içinde okunmamış varken kalın; açılınca bütün konuşma okundu olur | `docs/konusmalar.md` |
| Arama | VEYA / DEĞİL / parantez, yeni alanlar, göreli tarihler; arama kutusunda alan, değer ve kişi önerisi | `docs/arama.md` |

Test durumu: `cargo test --lib` 108 geçti (5 ignore), `npm run check` 0 hata / 15 uyarı, `npm run test:ui` 30 geçti.

## Yazma penceresi (2.0.0 ile yayımlandı)

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
`locales/*.json` (o tarihte 1649 anahtar).

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
