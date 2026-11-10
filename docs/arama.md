# Arama

Arama kutusuna yazılan metin arka uçta bir ifade ağacına çevrilir, ağaç SQL'e (yerel) ya da AQS'ye (Exchange
sunucu araması) dönüştürülür. Ön yüz yalnızca öneri ve gelişmiş arama formu için aynı anahtar tablosunu taşır.

## Neresi nerede

| Ne | Nerede |
|---|---|
| Sözdizimi çözümleyici, tarih, SQL üretimi | `src-tauri/src/db.rs`, "arama sözdizimi" bölümü |
| Anahtar adları (tr/en/takma ad → asıl ad) | `search_key()` |
| Sözcüklere ayırma (tırnak, parantez, `-`, VEYA) | `search_tokens()` → `Tok` |
| İfade ağacı | `SearchExpr` (And / Or / Not / Term), `SearchTerm` (Field / Sql / DateAfter / DateBefore / Folder) |
| Ayrıştırıcı | `SearchParser`, `parse_query_at(q, today)`, dışarıya `parse_query(q)` |
| Tarih değerleri | `parse_period()` (dönem → ilk/son gün), `date_terms()` (`a..b`, `>=`, `<` …) |
| SQL | `expr_sql()`; çağıran `filter_sql()` → `where_sql()` → `messages_page()` / `search_count()` |
| Adres → ad genişletme | `Db::expand_people()` |
| Tam ifade denemesi (birden çok sözcük) | `phrase_variant()` |
| Exchange sunucu araması metni | `search_to_aqs()`, `expr_to_aqs()`; çağıran `commands.rs` `search_server` |
| Öneriler (kişi, ek) | `Db::search_suggest()` → `search_suggest` komutu |
| Ön yüz anahtar tablosu, parça çözme, öneri | `src/lib/search-syntax.ts` (`OPERATORS`, `parts`, `current`, `hints`, `replaceCurrent`) |
| Arama kutusu | `src/lib/components/common/SearchBox.svelte` |
| Testler | `db.rs`: `search_language`, `search_expressions_filter_rows`, `word_prefix_search`; `tests/ui/search.spec.ts` |

**`search_key()` ile `search-syntax.ts`'teki `OPERATORS` aynı anahtarları taşımalı.** Birine anahtar ekleyince
ötekine de ekleyin; açıklaması için `locales/*.json`'a `search.op_<ad>` anahtarı gerekir.

## Sözdizimi

| Yazılan | Anlamı |
|---|---|
| `bütçe rapor` | iki sözcük de geçmeli (önek araması; büyük/küçük harf, İ/ı ve aksan gözetilmez) |
| `"aylık rapor"` | tam ifade |
| `fatura VEYA makbuz`, `fatura OR makbuz`, `a \| b` | seçenek (büyük harfle yazılır) |
| `-taslak`, `DEĞİL taslak`, `NOT taslak` | hariç tutma |
| `kimden:ali (fatura VEYA makbuz)` | parantezle gruplama |
| `konu:(bütçe rapor)` | anahtar bir gruba uygulanır |
| `kimden: ali` | iki noktadan sonra boşluk olabilir |

| Anahtar (tr / en) | Baktığı yer |
|---|---|
| `kimden` / `from`, `kime` / `to`, `bilgi` / `cc`, `gizli` / `bcc` | ilgili adres alanı |
| `kişi` / `participants` | gönderen, alıcı ve bilgi |
| `konu` / `subject`, `gövde` / `body`, `kategori` / `category` | ilgili alan |
| `ekadı` / `attachmentname` | ek adları |
| `ek:var`, `ek:yok` / `has:attachment` | eki olan / olmayan |
| `durum:` / `is:` | okunmamış, okunmuş, bayraklı, önemli, yanıtlandı, yanıtlanmadı, taslak, sabit |
| `önem:` / `importance:` | yüksek, normal, düşük |
| `klasör:` / `folder:`, `in:` | klasör adı ya da rolü (gelen, gönderilmiş, taslak, arşiv, silinmiş) |
| `tarih:` / `date:`, `on:` | o gün ya da dönem |
| `sonra:` / `after:`, `önce:` / `before:` | tarihten sonra / önce |

Tarih değerleri: `2026-03-15`, `15.03.2026`, `2026-03`, `2026`, `bugün`, `dün`, `bu hafta`, `geçen ay`, `bu yıl`,
ay adı (`mart`), göreli süre (`7g`, `2h`, `3a`, `1y`; İngilizce `7d`, `2w`, `3m`), aralık `2026-01-01..2026-03-31`,
karşılaştırma `tarih:>=2026-01-01`.

## Kurallar ve tuzaklar

- FTS dizini (`message_fts`) şu sütunları taşır: subject, sender, to_addr, cc, body_text, categories, att_names.
  **bcc dizinde yok**: `gizli:` her zaman `LIKE` + `lyn_match` ile aranır. `expr_sql` bir terimin bütün sütunları
  dizindeyse FTS, değilse `LIKE` kullanır; dizin henüz kurulmamışsa (`fts=false`) her şey `LIKE`.
- DEĞİL, SQL'de `(x IS NOT 1)` olarak yazılır (boş alanlı satırlar da eşleşsin).
- Kişi alanına tam adres yazılınca (`kimden:ali.veli@kurum.com`) `expand_people` adresi, `directory` ve `contacts`
  tablolarındaki adla VEYA'lar; ad bulunamazsa `ad.soyad@` → "ad soyad" tahmini yapılır. Neden: Exchange iç
  göndericide liste satırında yalnızca ad saklanır (adres X.500).
- Arama kutusunda alan yazılırken Enter **yazılanı** arar; öneri ↑↓ + Enter ya da Sekme ile alınır. Düz aramada
  ilk satır zaten "ara" satırıdır.
- Kişi önerisi alanın içindeyken (`kimden:ay`) adresi o alana yazar; alan yokken `kişi:<adres>` araması yapar.
- Gelişmiş arama formu sorguyu `parts()` ile alanlara ayırır; formda karşılığı olmayan parçalar (VEYA, parantez,
  `-sözcük`, `tarih:bu hafta`) "sözcükler" kutusunda olduğu gibi kalır.
