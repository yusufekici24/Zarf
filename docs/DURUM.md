# Durum

Bir oturumun sonunda güncellenir: son sürümde ne var, neye güvenilmez, sırada ne var. Yeni oturumda önce bu
dosya, sonra yalnızca ilgili konu belgesi okunur (`agent.md` → "Konu belgeleri").

## Son sürüm: 1.7.5 (2026-11-10)

| Konu | Yapılan | Ayrıntı |
|---|---|---|
| Tablolar | `tfoot` ve tablo öznitelikleri korunur; hücreler harf harf bölünmez; ölçülü resimler oranını korur | `docs/ileti-govdesi.md` |
| Konuşmalar | Bağ, Exchange konuşma kimliği ve `References`/`In-Reply-To` ile kurulur; aynı konulu ilgisiz iletiler birleşmez | `docs/konusmalar.md` |
| Konuşmada okundu | Baş satır içinde okunmamış varken kalın; açılınca bütün konuşma okundu olur | `docs/konusmalar.md` |
| Arama | VEYA / DEĞİL / parantez, yeni alanlar, göreli tarihler; arama kutusunda alan, değer ve kişi önerisi | `docs/arama.md` |

Test durumu: `cargo test --lib` 107 geçti (5 ignore), `npm run check` 0 hata / 15 uyarı, `npm run test:ui` 30 geçti.

## İlk açılışta olanlar (1.7.5'e yükseltmede bir kez)

- `subject_index_v2`: konu dizini (`ns`) yeniden yazılır.
- `threads_v1`: bütün iletilerin konuşması kurulur (100 bin iletide birkaç saniye).
- `ews_sync_v3`: Exchange klasörleri bir kez baştan sayılır; konuşma kimlikleri bu taramada dolar. Tarama
  bitene kadar eski Exchange iletileri konu yedek yoluyla bağlı görünür.

## Doğrulanmamış noktalar

- EWS konuşma özellikleri (`0x3013`, `0x0071`, `0x1042`, `0x1039`) yalnızca XML örneğiyle sınandı; gerçek
  Exchange'de ilk eşitlemeden sonra konuşmaların doğru birleştiğine bakılmalı.
- Yazma penceresinde alıntılanan tabloların görünümü (`RichEditor.svelte` CSS'i) gözle denenmedi.
- Paket yerelde derlendi; kurulu sistemde yükseltme (1.5.0 → 1.7.5) elle denenmedi.

## Sıradaki işler (istenirse)

- `gizli:` (bcc) araması dizinsiz; çok büyük kutuda yavaş olabilir. Gerekirse `message_fts`'e bcc sütunu.
- Arama kutusunda `klasör:` için gerçek klasör adlarının önerilmesi (şimdilik yalnızca roller).
- IMAP hesaplarında `X-GM-THRID` (Gmail) konuşma kimliği olarak kullanılabilir.
