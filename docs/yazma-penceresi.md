# Yazma penceresi ve düzenleyici

Yeni ileti / yanıt penceresi (`ComposeWindow.svelte`) ile biçimlendirmeli düzenleyicinin (`RichEditor.svelte`)
neresi ne yapar. Düzenleyici imza ve şablon düzenleyicilerinde, okuma bölmesindeki satır içi yanıtta da kullanılır.

## Dosyalar

| Dosya | İçerik |
|---|---|
| `src/lib/editor.ts` | `CaretState` (imleçteki biçim), `EditorApi` (araç çubuklarının çağırdığı komutlar), renk paletleri, paragraf stilleri (`BLOCK_STYLES`), tablo stilleri (`TABLE_STYLES`), `changeCase`, `textStats`, `normalizeUrl`, `portal` eylemi |
| `src/lib/editor-table.ts` | Tablo işlemleri (`tableOp`: satır/sütun ekle-sil, stil, genişlik) ve `applyTableStyle` |
| `src/lib/editor-history.ts` | Geri al / yinele geçmişi (`EditHistory`), yazma grubu kuralları (`inputKind`, `closesGroup`), seçim ↔ metin konumu (`readSelection`, `writeSelection`) |
| `src/lib/editor-colors.svelte.ts` | Renk düğmelerinin son rengi ve "son kullanılanlar" (bütün düğmeler paylaşır; `localStorage`) |
| `src/lib/components/common/RichEditor.svelte` | Yazı alanı, komutlar, imleç durumu, seçim araç çubuğu, bağlantı kartı, biçim boyacısı, klavye |
| `src/lib/components/editor/EditorTools.svelte` | Biçim düğmeleri, tek yerde. `layout`: `basic` / `font` / `paragraph` (şerit grupları), `bar` (düzenleyicinin kendi araç çubuğu), `mini` (seçim araç çubuğu) |
| `src/lib/components/editor/ColorButton.svelte` | Bölünmüş renk düğmesi + palet (`kind`: `fore` yazı, `back` vurgu, `shade` hücre gölgesi) |
| `src/lib/components/mail/ComposeWindow.svelte` | Pencere: şerit, başlık alanları, ek kartları, durum çubuğu, gönder/kaydet |
| `tests/ui/compose.spec.ts` | Arayüz testleri (pencere etiketi `w-compose`, içerik `window://assign` ile) |

## Kurallar

- **Renk için yerel `<input type="color">`'a `change` bağlamayın.** O olay yalnızca değer değişince gelir: aynı renk
  ikinci kez seçilince hiçbir şey olmuyordu (1.7.6'daki "ilk satır sarı oluyor, ikincisi olmuyor" hatası). Düğme son
  rengi doğrudan uygular; palette yerel kutu yalnızca özel renk seçmek içindir ve "Uygula" düğmesiyle çalışır.
- Biçim düğmeleri odağı almaz (`onmousedown` → `preventDefault`): seçim düzenleyicide görünür kalır. Odak başka
  yere gittiyse `restoreRange()` son seçimi (`savedRange`) geri yükler; komutlar bunu `exec()` üzerinden çağırır.
- İmleçteki biçim `readCaret()` ile okunur (`queryCommandState` + hesaplanan stil) ve `caret` (bindable
  `CaretState`) üzerinden şeride gider. Seçim boş değilse **seçili ilk metnin** biçimi okunur (`caretElement`).
- Punto: tarayıcı `<font size="7">` üretir, `fixFontSize()` bunu yerinde `style="font-size:Npt"` yapar (öğe
  değiştirilmez, seçim bozulmaz). `pendingSize` komuttan **önce** yazılır: komutun gönderdiği `input` olayı
  `fixFontSize`'ı hemen çağırır. Gönderirken `outgoingHtml` bu `<font style>` öğelerini `span` yapar.
- Paragraf stilleri (`BLOCK_STYLES`): düzenleyicide üretilen CSS kuralı (`blockStyleCss`), gönderirken aynı CSS
  satır içine (`inlineBlockStyles`, `outgoingHtml` içinde). Yalnızca `style` özniteliği olmayan ve alıntının
  (`.lynora-quote`) dışındaki öğelere uygulanır; girintinin ürettiği `blockquote`'un kendi stili vardır.
- Tablo stilleri hücrelerin satır içi stiline yazılır; anahtar `data-lyn-style` özniteliğinde durur ki satır/sütun
  eklenince başlık ve şerit renkleri yeniden hesaplansın (`restyle`). `insertHTML` kenarlık türünü satır içinden
  attığı için `insertTable` eklemeden sonra `applyTableStyle(…, "grid")` çağırır.
- Yüzen kutular (seçim araç çubuğu, bağlantı kartı) `portal` ile `<body>`'ye taşınır ve `data-editor-float` taşır.
  "Dışarı tıklama" sayılmayanlar: `[data-editor-float]`, `[data-slot$='-content']`, `[data-select-popup]`.
  `ComposeWindow` Esc'i bunlar açıkken pencereyi kapatmak için kullanmaz.
- Şeritteki "Tablo" sekmesi bağlamsaldır: yalnızca `caret.table` doğruyken listelenir (`ribbonTab`).

## Geri al / yinele

- Tarayıcının geri alma yığını kullanılmaz. WebKitGTK'de (2.52'de betikle denendi) **Ctrl+Z ve Ctrl+Y hiçbir şey
  yapmıyor**; DOM'u doğrudan değiştiren komutlar da o yığına girmiyordu. Düzenleyici kendi geçmişini tutar
  (`editor-history.ts`): her adımda `innerHTML` + seçim (metin konumu; `<br>`/`<img>` birer karakter).
- Kısayollar `RichEditor.onkeydown`'da: Ctrl+Z geri al, Ctrl+Y ve Ctrl+Shift+Z yinele (harfe bakılır; Türkçe F
  klavyede tuş yeri farklı). Düzenle menüsünden gelen `historyUndo`/`historyRedo` (`onbeforeinput`), şerit ve başlık
  çubuğu düğmeleri (`exec("undo"|"redo")`) aynı geçmişi kullanır.
- Yazarken her tuşta içerik seri hale getirilmez (büyük alıntılı yanıtta takılırdı): yazma grubu (`typing`) açık
  kalır; 1,2 sn ara, Enter, yazma ↔ silme geçişi ya da herhangi bir komut (`restoreRange` → `commitTyping`) grubu tek
  adım olarak kaydeder. Birden çok DOM değişikliği yapan komutlar `step()` ile tek adımdır (satır aralığı, paragraf
  stili, biçim boyacısı, tablo ekleme, tümünü değiştir).
- **Yeni komut DOM'u değiştiriyorsa** `restoreRange()` ile başlayıp `sync()` ile bitmeli (kayıt `sync`'tedir); birden
  çok adımlıysa `step()` içinde çalışmalı.
- Sınır: 200 adım ya da toplam 40 milyon karakter (satır içi resimli büyük iletide bellek şişmesin); aşılırsa en eski
  adımlar düşer. Dışarıdan gelen içerik (ilk yükleme) geçmişin başıdır; sonraki dış değişiklik (imza) adımdır.
- Metin kutuları (Kime, Konu, arama …) için `+layout.svelte`'teki pencere işleyicisi Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z'yi
  `document.execCommand("undo"|"redo")`'ya yönlendirir (WebKitGTK'de çalışır).

## Komutlar (`EditorApi`)

`exec` (execCommand sarmalayıcısı), `setFontSize`, `stepFontSize` (Ctrl+Shift+> / Ctrl+<), `setColor`,
`setBlock`, `setLineHeight`, `changeCase`, `link` (Ctrl+K: bağlantı kartı), `painter` (tek tıklama bir kez,
çift tıklama Esc'e kadar), `tableOp`. Ayrıca bileşenden: `insertTable`, `insertHtml`, `pickImage`, `stats`,
`findNext` / `replaceCurrent` / `replaceAll`, `getHtml`, `focus`.

Klavye: Sekme tabloda sonraki hücreye geçer (son hücrede yeni satır), listede girintiler, başka yerde dört
boşluk ekler. Esc önce seçim araç çubuğunu / biçim boyacısını kapatır.

## Ayarlar

| Anahtar | Değer | Nerede |
|---|---|---|
| `compose_mini_toolbar` | `1`, `0` | Seçenekler > Posta; `RichEditor.showBubble` |
| `compose_zoom` | `100` (50…200) | Durum çubuğundaki kaydırıcı; yalnızca yazı alanını büyütür, gönderilen iletiyi etkilemez |

## Bilinen sınırlar

- Birleştirilmiş hücreli tablolarda sütun ekleme/silme hücre sırasına göre yapılır.
- Sözcük sayısı alıntılanan özgün iletiyi saymaz.
- Testler Chrome'da çalışır; `execCommand` davranışı WebKitGTK'de farklı olabilir (bkz. `docs/DURUM.md`).

## Yapıştırılan resimler

- Resim gövdede `data:` adresiyle durur; gönderirken arka uç bunu `cid:` satır içi eke çevirir
  (`build.rs` `extract_data_images`, `body_part`: `multipart/alternative(text, multipart/related(html, resimler))`).
- `onpaste`: dosya yöneticisinden kopyalanan yollar (resim → gövde, diğerleri → ek), `clipboardData` içindeki resim
  dosyaları `FileReader` ile `data:`. **WebKitGTK ekran görüntüsünde `clipboardData` boştur** (2.52, `wl-copy
  --type image/png` ile denendi): işleyici bırakır, WebKit resmi `<img src="blob:…">` ekler ve `insertFromPaste`
  girdisi gönderir. `oninput` bu resimleri `settleImages()` ile `fetch` → `data:`'ya çevirir; geçmişteki adımlarda da
  adres değiştirilir (`EditHistory.replaceAll`), geri alınca geçersiz adres geri gelmez.
- `ComposeWindow.send` ve `saveDraft` önce `settleImages()` bekler; çevrilemeyen resim varsa gönderim sorulur
  ("Resim eklenemedi"). `navigator.clipboard.read()` WebKitGTK'de betikten izin vermez; şeritteki Yapıştır panoda
  metin yoksa `execCommand("paste")` kullanır.
