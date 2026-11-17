# Zarf 2.0 performans incelemesi — 17 Kasım 2026

## Sonuç ve kanıt sınırı

Şu an SQLite'ın değiştirilmesini veya veritabanının yeniden oluşturulmasını gerektiren
kanıt yok. Kaynak kodunda gereksiz bekleme bulundu ve kaldırıldı. Gerçek yavaşlığın
kesin kaynağı için yavaş çalışan makinenin ölçümü gerekiyor.

Bu ortamda varsayılan DB: 163.840 bayt, WAL: 671.592 bayt; 0 ileti, 0 ileti gövdesi,
0 kişi, 7 klasör. WAL açık, boş sayfa sayısı 0. Bu veriyle kullanıcının büyük posta
kutusunun yavaş olduğu sonucuna varılamaz. İçerik, parola ve kişi adresleri okunmadı.

## Sentetik sorgu ölçümü

`scripts/benchmark-db.py`: bellekte SQLite, 10 klasör, sentetik kayıtlar, mevcut
liste/sayaç/konuşma indeksleri. 11 çalıştırma, ilk çalışma hariç ortanca süreler (ms):

| Toplam ileti | İlk 500 satır | Tüm klasör sayaçları | Bir klasör konuşma grupları | Klasör adedi |
|---|---:|---:|---:|---:|
| 10.000 | 0,24 | 0,46 | 0,42 | 0,02 |
| 100.000 | 0,27 | 4,81 | 4,47 | 0,19 |
| 300.000 | 0,25 | 15,10 | 13,58 | 0,58 |

Bu bir disk/gerçek posta kutusu benchmark'ı değildir. Gövde, FTS, ağ, IPC, DOM,
senkronizasyon yazmaları ve SQLite bağlantı kilidi beklemesi ölçüme dahil değildir.
Konuşma ölçümü tam liste sorgusunun yalnızca GROUP BY aşamasıdır. Sonuçlar p95 değildir.

EXPLAIN: ilk liste `idx_messages_list` üzerinden SEARCH, sayaç tüm
`idx_messages_unread` indeksini SCAN, konuşma `idx_messages_conv` üzerinden SEARCH.
İlk 500 satır mevcut indeksle ucuz; sayaç ve konuşma gruplama toplam veriye göre büyür.

## Bulunan gecikmeler ve uygulanan düzeltmeler

1. `mail://changed` sonrasında önce `folder_tree` bekleniyor, ardından liste
   yenileniyordu. Böylece bütün posta kutusunun sayaç taraması seçili klasörü
   bekletiyordu. Liste yenilemesi sayaçtan bağımsız başlatılıyor.
2. Eşzamanlı `loadFolders` çağrıları aynı taramaları üst üste başlatabiliyordu.
   Tek etkin istek ve bu sırada değişiklik geldiyse bir sonraki yenileme kullanılıyor.
3. Kişi baş harf gruplaması O(n²) dizi kopyalıyordu; O(n) yapıldı. Arama 180 ms
   beklemeli ve eski yanıt korumalı. 100 kişilik sayfa DOM maliyetini sınırlar;
   DB'den kişi listesinin tamamının alınması hâlâ büyük rehberler için maliyettir.
4. Zamanlayıcı 50 üyelik sayfa, tekilleştirme, grup çözüm önbelleği kullanır.
5. `get_message` gövde eksikse indirmeyi, sonra MIME ayrıştırma ve HTML temizlemeyi
   bekliyor. Kullanıcının her iletiyi yeniden açışında gövde yeniden işlenebiliyor.
   İçerik önbelleği eklenmeden önce bunun süresi ölçülmeli; imza, uzak resim ve ek
   güvenlik durumları dikkate alınmadan hazır HTML'nin önbelleğe alınması doğru değil.

## Yavaş makinede tanı

Uygulama kapalıyken terminalden `ZARF_PERF=1 lynora` başlatın; açık örneğe ikinci
kez başlatmak ölçümü etkinleştirmez. Çıktıda kişisel içerik yazılmaz:

- `db_messages`: DB okuma bağlantısını bekleme, liste ve sayma toplamı.
- `message_content_wait`: gövdenin yerelde hazır olması / ağdan gelmesi için bekleme.
- `message_parse`: DB'den gövde okuma, MIME ayrıştırma ve HTML hazırlama.
- `ews`: toplam çağrı süresi, başarı, ServerBusy durumu; sunucu beklemesi dahil.
- Web Performance API: `zarf:startup`, `zarf:load-all`.

`python3 scripts/profile-db.py /veri/yolu/lynora.db` salt okunur metadata, sorgu planı
ve 11 tekrar zamanlaması üretir. SQLite DB'yi WAL dosyasından ayırarak kopyalamayın;
WAL'da henüz ana dosyaya geçmemiş kayıtlar bulunabilir. Betik tablo içeriği yazdırmaz.
Bu tanı tam FTS veya bozulma taraması yapmaz ve veri değiştirmez.

Aynı klasörde 20 geçiş, 20 yerel ileti açılışı, 20 önbelleksiz ileti açılışı ve
sabit arama sorgularını ayrı ölçün. Soğuk/sıcak ilk açılışı karıştırmayın. Eşitleme
kapalı/açık, konuşma kapalı/açık sonuçlarını ayrıca karşılaştırın.

## Ölçüme göre çözüm kararı

| Gözlem | Öncelikli çözüm |
|---|---|
| DB liste >100 ms, sayaç yüksek | Klasör bazlı değişiklikle sayaç yenileme / doğru invalidasyonlu sayaç önbelleği |
| Tarih liste hızlı, konu/gönderici sırası yavaş | Gerçek EXPLAIN'de geçici sıralama görülürse ilgili ifade/bileşik indeks; yazma maliyetini de ölç |
| Konuşma modu tek başına yavaş | Konuşma başlıklarını artımlı güncellenen özet tabloya taşı; eşitleme/silme/okundu testleriyle |
| DB hızlı, message_content_wait yüksek | Ağ, sunucu ve indirme kuyruğu; görünür komşu iletileri sınırlı ön yükleme |
| message_parse yüksek | Boyut sınırlı LRU; içerik değişimi ve güvenlik ayarı değişiminde geçersizleştirme |
| DB/ağ hızlı, çizim yavaş | WebKit profili, iframe/HTML boyutu, avatar istekleri, uzun görevler ve DOM sayısı |
| Eşitleme sırasında yavaş | Yazma transaction süreleri, WAL checkpoint ve okuyucu bekleme; toplu yazma boyutunu ölçerek ayarla |
| Disk alanı yüksek, freelist yüksek | Yedek sonrası mevcut sıkıştırma; rutin açılışta VACUUM yapma |

100 ms eşiği bir tanı önceliğidir, SLA değildir. Cache/indeks/özet tablo değişiklikleri
ölçüm olmadan topluca uygulanmadı. WAL kapatmak, FTS'yi gelişigüzel yeniden kurmak,
her açılışta VACUUM/ANALYZE yapmak veya SQLite'ı başka DB ile değiştirmek bu bulguların
önerdiği çözüm değildir. Önce gerçek yavaş işlem ve süre dağılımı belirlenmelidir.

## Bildirilen senaryoya özel öncelik: başka bilgisayarda açılış / arama / eşitleme

Koddan doğrulanan zaman çizelgesi: açılışta gerekirse DB geçişi beklenir; eşitleme
1,5 saniye sonra, yedek pencere 3 saniye sonra, eksik FTS dizinini tamamlama 8 saniye
sonra, ek adı geri doldurma 40 saniye sonra başlar. Büyük veya yeni taşınan posta
kutusunda bunlar aynı disk/CPU üzerinde çakışabilir. Bunlar doğrulanmış kod yollarıdır;
yavaş makinede hepsinin etkin olduğu henüz bilinmiyor.

Özellikle **FTS tamamlanmamışken arama dizinsiz yola düşer**. Tanı betiğindeki
`fts_build` boş değilse bunu ilk sırada inceleyin. Uygulamayı bir süre açık bırakıp
aynı sorguyu tekrar ölçün; ilerleme durmuşsa indeks kurma hatasını araştırın. FTS'yi
silerek yeniden kurmak başlangıçta yavaşlığı artırır. `--fts-term 'deneme'` doğrudan
FTS sorgu süresini içerik yazdırmadan ölçer; uygulamanın tam arama ifadesiyle aynı test
değildir. Boş olmayan `fts_build` durumunda bu sorgu yalnızca dizinlenmiş kısmı sayar.

Arama kutusunda zaten 250 ms bekleme var. Çok sözcüklü arama önce tam ifade var mı
sorguluyor; ayrıca yerel sonuçlardan sonra sunucu araması başlıyor. “Sonuçlar geldi
ama gösterge sürüyor” ile “yerel liste hiç gelmiyor” ayrılmalı. İlki Exchange araması,
ikincisi FTS/SQL/okuyucu beklemesi veya çizim olabilir.

Önerilen karşılaştırma: aynı veriyle açılışta otomatik eşitlemeyi geçici kapatarak
ilk açılışı ve aynı aramayı karşılaştırın; sonra ayarı geri açın. Belirgin fark varsa
önce eşitleme/indeksleme işlerini ilk ekran hazır olana kadar erteleyen ve kullanıcı
etkinliğinde yavaşlatan zamanlama değerlendirilmelidir. Aynı anda daha çok istek
atmak Exchange ServerBusy sorununu artırabilir; paralellik ilk çözüm değildir.

Bu makinede yavaş posta kutusu bulunmadığı için bu karşılaştırma yapılmış gibi
raporlanmadı. Kesin DB müdahalesi, yavaş bilgisayarın tanı çıktısından sonra seçilmelidir.
