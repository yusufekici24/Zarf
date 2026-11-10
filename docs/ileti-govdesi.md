# İleti gövdesi ve tablolar

HTML gövde iki aşamadan geçer: arka uçta temizlenir (ammonia), ön yüzde korumalı iframe'e yazılmadan önce
düzeni ayarlanır.

## Neresi nerede

| Ne | Nerede |
|---|---|
| HTML temizleme, izinli etiket ve öznitelikler | `src-tauri/src/mail/html.rs` (test: `sanitize_keeps_table_structure`) |
| iframe belgesi, taban CSS, CSP | `src/lib/components/mail/MessageBody.svelte` `doc` |
| Tablo ve resim düzeni ayarı | `MessageBody.svelte` `tune()`, birim çevirici `px()` |
| Yükseklik ölçümü (`autoHeight`) | `MessageBody.svelte` (dışarıdan zamanlayıcıyla; yatay kaydırma çubuğu payı eklenir) |
| Yazma penceresinde alıntılanan tablo ve resim | `src/lib/components/common/RichEditor.svelte` (Tailwind sıfırlamasını geri alan CSS) |
| Testler | `tests/ui/tables.spec.ts` |

## Kurallar

- `tfoot`, ammonia'nın varsayılan listesinde yok: atılırsa toplam satırı tablonun başına kayar. İzinli etiketlere
  eklendi. Tablo öznitelikleri `bordercolor`, `rules`, `frame` de korunur.
- Gövdede `overflow-wrap: break-word` kullanılır (`anywhere` değil): `anywhere` hücrenin en küçük genişliğini bir
  harfe indirir ve dar bölmede sütunlar harf harf alt alta dizilir.
- Boşluksuz uzun metin (36+ karakter; adres, kimlik) taşıyan hücreye tek tek `overflow-wrap: anywhere` ve
  genişliği yoksa `min-width: 10em` verilir. `nowrap` hücreye dokunulmaz.
- Tablo içindeki ölçülü resim küçültülmez (`max-width: none`): imza ve bülten tabloları sütun genişliğini
  resimden alır; bölme darsa tablo yatay kayar.
- Tablo dışındaki, eni ve boyu verilmiş resim daralınca oranını korur (`aspect-ratio` + `height: auto`).
- Tailwind sıfırlaması (preflight) tablo kenarlığını, hücre boşluğunu ve resim boyutunu siler. iframe bundan
  etkilenmez; yazma düzenleyicisi etkilenir, bu yüzden `RichEditor` kuralları geri koyar.

## Tuzaklar

- WebKitGTK'de korumalı iframe'in içine eklenen dinleyici ve gözlemciler çalışmaz; ölçüm dışarıdan yapılır.
- `tests/ui` dosyalarında `process` kullanılmaz (`npm run check` hata verir).
