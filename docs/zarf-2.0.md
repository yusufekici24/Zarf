# Zarf 2.0 — kişi yönetimi, zamanlayıcı ve performans

17 Kasım 2026. Bu değerlendirme kaynak kodu incelemesine, yerel testlere ve aşağıdaki
mikro ölçüme dayanır. Gerçek Exchange sunucusu üzerinde gecikme/CPU ölçülmedi.

## Kişilerde ekleme ve güncelleme olmalı mı?

Evet: kişisel adres defterinde kişi ekleme ve mevcut kişinin ad, e-posta, telefon,
kurum ve not bilgilerini güncelleme temel işlevlerdir. Uygulamada zaten var:
`ContactsView.svelte` içindeki Yeni Kişi, düzenleme penceresi ve `saveContact` akışı;
arka uçta `save_contact` ile yerel kayıt ve CardDAV hesabı varsa sunucuya yazma.
Exchange kişi güncellemesini sunucuya gönderen bir akış bu komutta yok.
Bu yüzden ikinci bir kullanıcı ekleme ekranı eklenmedi.

Kişi kaydı oluşturmak bir e-posta hesabı veya Active Directory kullanıcısı açmak değildir.
Kurum adres defteri/GAL kayıtları kaynak dizin tarafından yönetilmelidir. Bunları
kişisel kişi kaydıyla karıştıran bir “kullanıcı güncelle” düğmesi doğru olmaz.
Yerel/CardDAV kayıtları düzenlenebilir. Exchange kayıtları düğme, çift tıklama ve
arka uçta salt okunurdur; kişisel kopya oluşturulabilir. Kayıt kaynağı gösterilir.
Yeni kayıtta aynı e-posta bulunursa ayrı kayıt oluşturmadan önce onay istenir.
Kişiler 100 kayıtlık, zamanlayıcı 50 üyelik sayfalarda çizilir; ortak uygunluk tüm
üyeler üzerinden hesaplanır. CardDAV aktarımının kayıt bazında kalıcı hata gösterimi
henüz yoktur; genel hata akışı korunmuştur.

## Zamanlayıcı düzeltmesi

Davet alanı grup adresini korur. Zamanlayıcı açıldığında grup üyeleri ayrı satırlara
çözümlenir; iç içe gruplar desteklenir. Adresler küçük harfle tekilleştirilir ve zorunlu
katılım isteğe bağlı katılımın önüne geçer. Satırda kaynak grubun adı görünür.

- Yerel kişi grupları (`@` içermeyen adlar dahil) ve Exchange dağıtım listeleri kullanılır.
- Grup tespiti 200 adreslik parçalarla; boş/meşgul arka uçta mevcut 100 adreslik parçalarla yapılır.
- Döngü, 10 iç içe grup ve toplam 2000 üye açılımı sınırı vardır. Boş/erişilemeyen/sınıra
  takılan grup uyarıyla ve bilinmeyen müsaitlikle kalır; sessizce atılmaz.
- Aynı davetli listesiyle gün değiştirirken grup çözümü yeniden kullanılabilir. Yenile
  düğmesi grup üyelerini de tekrar getirir. Önbellek yalnızca pencere ömründedir.
- Eski ağ yanıtları yeni günün veya davetli listesinin üzerine yazamaz.
- Ayrıntılar paylaşılmasa bile `merged` meşguliyet rengi gösterilir.
- Bilinmeyen müsaitlik otomatik seçimde boş sayılmaz; yüklenirken de bilinmeyendir.

Gerçek sunucuda izinli/izinsiz gruplar, çok hesaplı kullanım ve Exchange sürümleriyle
kabul testi gerekir. Boş/meşgul sorguları seçili takvimin hesabını kullanır; hesapsız takvimde ilk uygun Exchange hesabı seçilir.

## Uygulanan performans iyileştirmeleri

| Bölüm | Önce | Şimdi |
|---|---|---|
| Kişileri baş harfe göre gruplama | Her kayıtta birikmiş diziyi kopyalama; aynı harfte O(n²) | Yerel Map kovasına push; O(n) |
| Kişi araması | Her değişimde sorgu, eski yanıt yeni sonucu ezebilir | 180 ms bekleme, son isteğin sonucu uygulanır |
| Kişiler/gruplar yükleme | Art arda iki IPC çağrısı | Birlikte başlatılır, sonuç birlikte uygulanır |
| Zamanlayıcı | Her davet değişiminde hemen sorgu | 200 ms bekleme, tekilleştirme, pencere içi grup çözümü önbelleği |
| Açılış | Sürekli logo nabzı | Tek giriş animasyonu ve hafif ilerleme hareketi; hareket azaltma tercihi destekli |

Yerel Node mikro ölçümü: aynı baş harfli 20.000 kişi, 7 tekrarın ortancası.
Eski dizi kopyalama **2748,13 ms**, yeni kova yaklaşımı **2,80 ms**.
Bu sentetik en kötü dağılıma yakın bir gruplama ölçümüdür; bütün uygulamanın bu oranda
hızlandığı anlamına gelmez. Ekrana çizim ve ağ süresi bu ölçümde yoktur.

## Performans ölçümü ve gerçek ortam kabulü

1. Kişi listesi 100 kayıtlık sayfalara ayrıldı. Önceki durum: `ContactsView` bütün sonuçları DOM'a çizer;
   posta listesinde zaten sanal kaydırma vardır. 1k/10k/50k kişiyle ilk çizim ve bellek ölçülmeli.
2. Zamanlayıcı 50 üyelik sayfalara ayrıldı. Önceki durum: 2000 × 48 dilim büyük DOM oluşturabilir.
   Önce 50/200/1000 üyede sekme açılışı ve kaydırma ölçülmeli.
3. Exchange ağ ölçümü: istek sayısı, süre, kuyruk bekleme ve ServerBusy sıklığı ayrı
   izlenmeli. Mevcut sunucu bekleme kapısı korunmalı; paralellik körlemesine artırılmamalı.
4. Veritabanı: WAL, 3 okuyucu, liste indeksleri ve FTS zaten var. Gerçek yavaş sorguların
   EXPLAIN QUERY PLAN çıktısı alınmadan indeks veya PRAGMA değiştirilmemeli.
5. Açılış: veri geçişi, app.loadAll, ilk posta listesi ve gizli yedek pencere maliyetleri
   ayrı ölçülmeli. Soğuk/sıcak açılışın medyan ve p95 değerleri kaydedilmeli.

## Marka ve yükseltme

Görünen ürün adı Zarf, sürüm 2.0.0. Vektör kaynak `static/icon.svg`; paket simgeleri
bu kaynaktan `npm run tauri icon static/icon.svg` ile üretildi. Yerel GTK açılış ekranı
aynı vektörü, gerçek paket sürümünü ve daha yumuşak ilerleme hareketini kullanır.

Veri dizini `~/.local/share/lynora`, veritabanı, anahtarlık hizmeti `Lynora`, uygulama
kimliği `app.lynora.Lynora`, çalıştırılabilir dosya/crate `lynora`, mevcut IPC ve ayar
anahtarları korundu. Debian paket adı `zarf` oldu; `Conflicts/Replaces/Provides: lynora, mail-nix`
eski paketin yerini almasını sağlar. Veri kimlikleri kullanıcıya görünen marka değildir; değiştirmek hesap,
parola, varsayılan uygulama ve eski yedek uyumluluğu için ek geçiş gerektirirdi.

Paketin masaüstü dosyası `Zarf.desktop` olur; eski `Lynora.desktop` varsayılan dosya
atamaları ilk açılışta yeni masaüstü kimliğine aktarılır. Yeni ve eski kimlikler
varsayılan uygulama kontrolünde tanınır; çalıştırılabilir dosya ve simge adı `lynora` kalır.

## 2.0 yayın doğrulaması

Logo ve açılış ekranı kırmızı-gri. `ZARF_PERF=1 lynora` ile EWS çağrı süresi,
başarı ve ServerBusy bilgisi stderr'e yazılır (adres, gövde veya parola yazılmaz).
Web Performance API'de `zarf:startup` ve `zarf:load-all` ölçümleri vardır. Gerçek
sunucu gecikmesi ve soğuk/sıcak açılış p95 ölçümü kurumsal test ortamında yapılmalıdır.
Üretim verisi olmadan tahmini indeksler eklenmedi; mevcut FTS/WAL yapısı korundu.
