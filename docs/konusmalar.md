# Konuşmalar (bağlantılı iletiler)

Bir konuşma, `messages.tid` değeri aynı olan iletilerdir. `tid` ileti eklenirken hesaplanır; konu yalnızca
yedek yoldur.

## Neresi nerede

| Ne | Nerede |
|---|---|
| Sütunlar `messages.conv` (Exchange konuşma kimliği), `messages.tid` | `db.rs` `MIGRATIONS` |
| `thread_keys (account_id, key, tid)` tablosu ve dizinler | `Db::init_threads()` |
| Var olan veritabanını bir kez kurma (`threads_v1` ayarı) | `Db::rebuild_threads()` |
| Bir iletiyi konuşmaya bağlama | `Db::assign_thread(c, id)` |
| İletinin anahtarları | `thread_keys(conv, mid, in_reply_to, refs, ns)` |
| Sonradan gelen konuşma kimliği | `Db::set_conversation()`; çağıran `ews_sync.rs` `ews_insert_headers` |
| Çöp ve gereksiz klasörlerin dışarıda tutulması | `THREAD_SCOPE` |
| Konu yedek yolunun süresi (90 gün) | `SUBJECT_THREAD_WINDOW` |
| Konu önekleri (RE, YNT, İLT …) ve `ns` | `mail/mod.rs`: `SUBJECT_PREFIXES`, `normalize_subject()`, `has_subject_prefix()` |
| Liste sorgusu (konuşma başına bir satır) | `Db::messages_page()` |
| Bir konuşmanın iletileri | `Db::conversation(m)` |
| EWS başlık özellikleri | `mail/ews.rs`: `HEADER_PROPS`, `EwsHeader::from_element` |
| Ön yüz okundu mantığı | `stores/mail.svelte.ts`: `unread`, `hiddenUnread`, `select`, `show`, `autoRead`, `setRead` |
| Liste satırı | `components/mail/MessageList.svelte` (`head` satırında `read = !mail.unread(m)`) |
| Testler | `db.rs`: `conversation_by_normalized_subject`, `threads_follow_reply_headers`, `paged_sorted_and_conversations`, `rebuild_threads_speed` (ignore); `ews.rs`: `header_reads_conversation_properties`; `tests/ui/conversation.spec.ts` |

## Bağlama kuralları

1. Exchange iletisinde konuşma kimliği varsa anahtar `x:<conv>` olur. Kimlik `PR_CONVERSATION_ID` (0x3013)
   özelliğinden, yoksa `PR_CONVERSATION_INDEX`'in (0x0071) 6–22. baytlarından alınır.
2. Kimlik yoksa `References` ve `In-Reply-To` zincirindeki her ileti kimliği için `m:<kimlik>\x1f<ns>` anahtarı
   kullanılır (24'ten uzun zincirde ilk 4 + son 20). Anahtara `ns` de girdiği için konusu değiştirilen yanıt ayrı
   konuşma olur.
3. Her ileti kendi `Message-ID` anahtarını da yazar; sonradan gelen yanıt onu bulur.
4. Başlığı olmayan iletide konu yedek yolu: yalnızca `conv=''` ve öneki olan (RE:, YNT: …) ileti, aynı hesapta
   aynı `ns`'li ve 90 gün içindeki daha eski başlıksız iletiye bağlanır. **Öneksiz aynı konulu iletiler
   birleşmez** (her gün gelen "Günlük rapor" ayrı kalır).
5. Bir ileti birden çok konuşmaya anahtar veriyorsa konuşmalar en küçük `tid`'de birleşir.
6. Yeni `tid` = `messages` ve `thread_keys`'teki en büyük `tid` + 1.

Gövde sonradan gelip konu değişirse (`update_message_content`) ileti konuşmadan ayrılır (eski `ns`'li kendi
anahtarı silinir, `tid=NULL`) ve yeniden bağlanır.

## Liste ve sayılar

- `MessageRow.thread_id` = `tid`. `thread_count` klasörler arası sayıdır (gönderilmiş yanıtlar dahil; çöp ve
  gereksiz, yalnızca o klasöre bakılıyorsa dahil). `thread_unread` yalnızca listelenen klasördeki okunmamışlardır.
- Konuşmanın baş satırı, içinde okunmamış ileti varsa kalın görünür (`mail.unread`).
- Baş satırı açmak o klasördeki bütün okunmamışları okundu yapar (`show(id, whole=true)`); açılmış konuşmada
  alt satıra tıklamak yalnızca o iletiyi okur (`select(id, e, child=true)`).
- "Okundu işaretle" baş satırda bütün konuşmayı kapsar (`setRead(ids, value, whole=true)`); sayı iyimser olarak
  `thread_id` üzerinden güncellenir.

## Açılış geçişleri (ayar anahtarı ile bir kez)

| Anahtar | Ne yapar |
|---|---|
| `subject_index_v2` | `ns` sütununu yeni önek kurallarıyla yeniden yazar |
| `threads_v1` | bütün iletilerin `tid`'sini kurar (100 bin ileti: hata ayıklama derlemesinde ~5 sn) |
| `ews_sync_v3` | Exchange klasörlerinin `sync_state`'ini sıfırlar: sonraki eşitleme başlıkları yeniden sayar ve konuşma kimliklerini doldurur (`set_conversation`) |

## Tuzaklar

- EWS genişletilmiş özellikleri yalnızca XML örneğiyle sınandı; kurum sunucusuna deneme isteği atılmaz.
- `set_conversation` yalnızca `conv=''` satırda çalışır; dolu kimliğin üzerine yazmaz.
- Sahte arka uçta (`tests/ui/mock-backend.ts`) konuşma `ns` eşitliğiyle taklit edilir; `thread_id` aynı `ns`'li en
  küçük kimliktir.
