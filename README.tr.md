<div align="center">

# simcraft

**Kendi fizik motoruna sahip, masaüstünde ve telefonunda çalışan deterministik bir simülasyon ve oyun motoru.**<br>
Oyunlar okunabilir veri dosyalarıdır: çalışmadan önce denetlenir, bit bit tekrar oynatılır, adım adım ölçülür, yapay zekâyla yapılır.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org)
[![Agent Skills](https://img.shields.io/badge/AI-agent%20skills-8A2BE2.svg)](skills/)

[Başlangıç](#başlangıç) · [Özellikler](#özellikler) · [Oyunlar](#öne-çıkan-oyunlar) · [Mobil](#mobil-sim-mobile) · [Mimari](#mimari) · [Kimler için](#kimler-için) · [Belgeler](docs/architecture.md) · [Katkı](CONTRIBUTING.tr.md) · [English](README.md)

</div>

<p align="center">
  <img src="docs/media/race_chase.jpg" alt="games/race: eğimli bir ovalde sekiz stock car" width="100%">
</p>
<sub><code>games/race</code>: 1.5 millik eğimli bir ovalde stock car yarışı. Her araba veridir ve simcraft'ın kendi araç
fiziğiyle 480 Hz'de adımlanır. Yedisi kendini sürer, sekizincisini sen.</sub>

## Tasarım ilkeleri

- **Deterministik simülasyon.** Baştan sona tam sayı aritmetiği (simülasyonda float yok), sabit zaman adımı,
  paylaşılan rastgelelik yok, hash-map sırasına bağlılık yok. Aynı tohum ve girdiler her makinede ve her çekirdek
  sayısında aynı dünyayı verir. Koşular bit bit tekrar oynar; her deney kanıttır. (Bilerek yapılmış tek istisna:
  yeni 3D katı cisim çözücüsü, ilk oyununun hissi ayarlanırken `f32` ile çalışır ve aynı derlemede deterministiktir;
  his oturunca sabit noktaya geçecek. Kural motorunun dışında durduğu için kural oyunları etkilenmez.)
- **Önce veri.** Bir oyun, bir `game.ron` (türler, durum makineleri, kurallar, eylemler) ve bir `engine.toml`'dır
  (tohum, nüfus, parametreler). Araba bir araç dosyası, pist bir pist dosyasıdır. Tasarımcı kodu değil sayıları
  değiştirir.
- **Çalışmadan önce denetlenir.** Her ifade yüklemede derlenir ve kuru çalıştırılır. Tüm hatalar tek seferde, ait
  oldukları kural, durum veya alanla, bir insanın ya da yapay zekânın düzeltebileceği sözlerle listelenir.
- **Varsayılmaz, ölçülür.** Fizik kapalı form cevaplara karşı test edilir. Performans kayıtlı taban ölçümlere karşı
  ölçülür ve her adımda dünyanın hash'i karşılaştırılır. Oyun hissi de kameranın gerçekte çizdiğini ölçen bir
  sondayla ölçülür.
- **Tek oyun, masaüstünden telefona.** Aynı çekirdek pencerede, testlerde pencere olmadan ve bir kez derlenen bir
  telefon uygulamasında çalışır. Oyun, oynatıcının yüklediği veridir; bir değişikliği gerçek telefonda denemek bir
  dakikadan kısa sürer.
- **Dar bir sınırın arkasında fizik.** `sim-physics` oyunları ve dünyayı bilmez: düz veri girer, sabit bir adım
  atılır, düz veri çıkar. Unity, Unreal ve Godot'nun fizik kütüphaneleriyle yaptığı ayrımın aynısı.

## Özellikler

### Fizik (`sim-physics`)

- **Sayısal temel:**
  - 128 bit ara değerli Q48.16 sabit nokta ve tam tamsayı karekök;
  - ikili açılar (tur başına 2³²);
  - derleme anında tam sayı matematiğiyle kurulan sinüs/kosinüs tablosu;
  - CORDIC ile `atan2`.
- **Araç dinamiği:**
  - kayma açılı lastiklerle dinamik bisiklet modeli (sınıra kadar doğrusal, sonra kayar);
  - itiş, fren ve viraj arasında paylaşılan sürtünme çemberi;
  - yük transferi, downforce ve eğim;
  - yürüme hızında kinematik model.
- **Aktarma organları:**
  - dyno noktalarından okunan tork eğrisi;
  - kaydıran debriyaj, devir sınırlayıcı ve motor freni;
  - optimum vites noktalarında otomatik ya da elle kullanılan sıralı şanzıman;
  - geri vites.
- **Sürüş yardımları:** çekiş kontrolü ve ABS.
- **Zaman adımı:** 60 Hz tick, her tick'te 8 alt adım (480 Hz), yarı-örtük Euler.
- **Çarpışma:**
  - sweep-and-prune geniş faz;
  - yönlü kutular arasında ayırma ekseni testiyle dar faz (ortalamalı temas manifoldu);
  - birikimli kırpmalı ardışık impulslar, geri sekme ve Coulomb sürtünmesi;
  - pistin kenarlarından duvarlar.
- **Katı cisimler (3D):** üst üste duran, dinlenen, devrilen ve vurulan kutular, dik prizmalar (kutu, kavanoz) ve
  küreler:
  - 6 alt adımlı Box2D v3 soft step, warm starting, relax ve geri sekme geçişleri, Coulomb sürtünmesi, yuvarlanma
    direnci;
  - dışbükey gövdelerde ayırma ekseni testi ve yüz kırpma (en fazla 4 temas noktası);
  - hızlı bir merminin içinden geçemediği spekülatif temaslar; yalnızca süpürme gerçekten ulaşıyorsa tutulur
    (hayalet çarpışma yok);
  - birlikte uyuyan adalar: uyur halde doğan bir kule, vurulana kadar bit bit aynı kalır;
  - çarpışma katmanları (kuleye değil yere çarpan enkaz), kırılma, titreşim ve kamera için darbeler, ışın ve tam
    küre atışı (sphere cast).
- **Veri olarak pistler:** eğimli düzlükler, dairesel yaylar ve klotoid (geçiş spirali) ile yumuşatılmış virajlar.
  `pose(s, offset)` ve `locate(x, y)` bir şeyin nerede olduğunu söyler.
- **Sürücü yapay zekâsı:**
  - yarı-durağan tur planı;
  - understeer telafili pure-pursuit direksiyonu;
  - yarış zekâsı (geçiş, hıza bağlı mesafede takip);
  - spin kurtarma.

### Mobil (`sim-mobile`)

- **iPhone ve Android için tek kabuk:** winit ve wgpu (Metal, Vulkan); uygulamanın yaşam döngüsüyle bırakılıp
  yeniden kurulan yüzeyler; dönmek yerine ekranı bekleyen bir döngü (duran ekranda %94 → %2–3 CPU).
- **Bir kez derlenen oynatıcı:** uygulama simcraft oyunlarını veri olarak oynar (paketlenmiş `game.ron` bölümleri ya
  da bir oyunun ayar dosyası); bir oyunu yayımlamak motoru derlemeyi gerektirmez.
- **Dokunma:** dokunma, sürükleme, kaydırma, çek-bırak ve bir şeklin üstünden geçen parmak (ipler için).
- **Titreşim bir sözlük olarak:** tap, thud, tick, rise, fall ve buzz; yoğunluk ve keskinlikle. iOS'ta Core Haptics.
- **Sensörler:** Core Motion'dan eğim, tam sayı olarak; tekrarlar bunu da içerir.
- **Ekran katmanları:** arka plan, tahta, parçalar, efektler, HUD ve üst katman; güvenli alana (çentik, ada, ana
  çubuk) bağlı.
- **Telefonda 3D:** güneş gölge haritası, yarım küre ışığı, her kutu kenarında oyuncak görünümlü pah, sis ve 4×
  MSAA ile örneklenmiş modeller; 2D katmanların altında.
- **Derle ve çalıştır:** `tools/mobile/build.sh ios phone` Wi-Fi üzerinden iPhone'una kurar; simülatör, App Store
  arşivi, Android APK ve Play paketi tek komut.

### Simülasyon (`sim-core`, `sim-state`, `sim-rules`)

- **Dünya:** ızgara dünya (2B ya da voksel), üstünde sürekli hareket (hücre altı konum ve hız, yerçekimi, yolcu
  taşıyan binekler), ayak izi sorguları ve araçlar.
- **Davranış:** iç içe, paralel katmanlı, yeniden kullanılabilir durum şemaları; remember, interrupt/back ve pick
  (Unity Animator ve Unreal StateTree'nin dağarcığı).
- **Kurallar:** veri olarak, bir kez derlenen kurallar. Ortak alt küme için yerel bir değerlendirici var (closure
  compilation), gerisi [Rhai](https://rhai.rs).
- **Sorgular:** hareketli türler için sweep-and-prune indeksi.
- **Durum:** anlık görüntü ve geri yükleme, tekrar kaydı, canlı izleme için mesaj veri yolu.

### Görüntü ve oynanış (`sim-gpu`, `sim-render`)

- **wgpu ile GPU çizici:**
  - 2.5B sahne görünümü;
  - birinci şahıs pist görünümü;
  - birinci şahıs voksel görünümü;
  - sürüş görünümü: kokpit, takip ve tepeden kameralar, canlı ayna, liverilerle çoğaltılmış glTF modelleri,
    fotoğraf malzemeler.
- **Veri olarak kamera hissi ve girdi:**
  - g kuvvetleriyle eğilen yaylı baş;
  - analog klavye eksenleri;
  - döngülü tuşlar.
- **Ses:** veri yollu bir mikser. Motor döngüleri devire göre çapraz geçişlidir, diğer arabalara uzaklık zayıflaması
  ve Doppler uygulanır, telsizde bir spotter anons yapar.
- **Terminal çizici:** ASCII ve piksel sanat.

### Araçlar

- **Ajan protokolü:** insanlar, betikler ve yapay zekâ ajanları için aynı JSON satır protokolü; host motorlar için
  bir C API (`sim-ffi`).
- **Ölçüm araçları:**
  - `tools/eval.py`: kural oyunları için eval güdümlü geliştirme; `simcraft-smash eval`: SMASH için;
  - `tools/perf.py`: dünya hash kontrollü ölçeklenme ölçümleri;
  - `simcraft-physics-bench`: fizik ölçüm sahneleri;
  - `--feel`: his sondası;
  - `simcraft-play --shot` ve `simcraft-smash shot`: pencere açmadan kare çizer, bir kamera değişikliği betikten
    görülüp karşılaştırılabilir.
- **Yapay zekâ ajan becerileri** ([`skills/`](skills/)).

## Öne çıkan oyunlar

### `games/race`: eğimli ovalde stock car (masaüstü)

Charlotte Motor Speedway'in yayımlanmış uzunluğu, viraj yarıçapları, viraj uzunlukları ve 24° eğimiyle 1.5 millik
bir quad-oval (dogleg'in şekli yaklaşık). 8 arabalık alanın en arkasından başlarsın.

```bash
cargo build --release -p sim-gpu && target/release/simcraft-play games/race
```

| Tuş | |
|---|---|
| ↑ ↓ ← → | gaz, fren, direksiyon (klavyeden analog rampalar) |
| **G** ya da MODE düğmesi | **CONTROL** (direksiyon sende; çekiş kontrolü ve ABS) → **GUIDED** (çizgiyi sen seçersin, araba onu izler ve seni tutuşunun içinde tutar) → **AUTOPILOT** |
| C · A / Z · Backspace | kamera (kokpit, takip, tepeden) · vites yukarı / aşağı · geri |
| Tab · M · R | başka arabaya bin · sessiz · yeniden başla |

#### Nasıl yapıldı

Motorun hiçbir yeri bunun bir yarış olduğunu bilmez. Ekrandaki her şey birkaç satır veriden gelir; genel
mekanizmayı motor sağlar: fizik, pistler, kameralar, girdi. Her görüntüyü neyin ürettiği aşağıda.

<table>
<tr>
<td width="44%"><img src="docs/media/race_chase.jpg" alt="virajda sürü"></td>
<td>

**Eğimli virajda sekiz araba.** Her araba bir dosyadaki teknik veri sayfasıdır:
[`assets/vehicles/stock_car.ron`](assets/vehicles/stock_car.ron). İçinde kütle, dingil mesafesi, dyno noktalarıyla
tork eğrisi, vites oranları, frenler, lastik tutuşu, sürtünme ve downforce var. Oyun yalnızca hangi türün araba
olduğunu söyler: `vehicle: "stock_car"`. Fizik motoru bu sayıları hıza, tutuşa, kaymaya ve vites değişimine
çevirir. Bir kamyon ya da hatchback başka bir koddur değil, başka bir dosyadır.

</td>
</tr>
<tr>
<td><img src="docs/media/race_grid.jpg" alt="tepeden başlangıç ızgarası"></td>
<td>

**Başlangıç ızgarası.** Pist, eğimleriyle düzlük ve virajlardan oluşan bir dosyadır:
[`games/race/tracks/charlotte.ron`](games/race/tracks/charlotte.ron), pistin yayımlanmış verilerinden üretildi.
[`game.ron`](games/race/game.ron)'daki bir kural ilk tick'te çalışır: her arabaya bir ızgara yeri, bir tempo ve bir
çizgi verir; yedisine motorun otopilotunu. Renk şemaları [`drive.ron`](games/race/drive.ron)'da tek bir 3B modeli
boyayan bir listedir.

</td>
</tr>
<tr>
<td><img src="docs/media/race_cockpit.jpg" alt="kokpit"></td>
<td>

**Koltuktan.** Kokpit, `drive.ron`'da bir tariftir: sürücünün gözü nerede, boyun yayları ne kadar sert, ayna nerede.
Fizik motoru sürücünün hissettiği g kuvvetlerini bildirir, baş onlara karşı eğilir; gösterge motorun devrini ve
vitesini gösterir. Aynı kamera arabası olan her oyunda çalışır.

</td>
</tr>
<tr>
<td><img src="docs/media/race_line.jpg" alt="GUIDED modu, yoldaki çizgi"></td>
<td>

**GUIDED: çizgiyi sen seçersin.** Üç oyun modu, arabandaki tek bir sayıdır: kim sürüyor (sen, otopilotla sen, ya da
otopilot). Sola ya da sağa bastığında bir kural çizgini kaydırır, motorun otopilotu da onu izler. Yoldaki soluk
çizgi ve MODE düğmesi, `drive.ron`'da o sayıyı gösteren iki satırdır.

</td>
</tr>
<tr>
<td><img src="docs/media/race_banner.jpg" alt="başlangıç/bitiş çizgisi"></td>
<td>

**Başlangıç/bitiş.** Pankart, seyirciler, SAFER bariyeri ve asfalt, `drive.ron`'da adı geçen resimlerdir. Turlar ve
tur süreleri `game.ron`'daki iki kısa kuraldır: çizgiyi geçmek bir tur sayar, kural en iyisini hatırlar.

</td>
</tr>
</table>

| Ölçülen | Sonuç |
|---|---|
| Next Gen arabayla otopilot turu | **30.75 s**, 2024'ün gerçek pole'u **29.355 s** ([her adımın kaydı](games/race/LAPS.md)) |
| Kapalı form cevaplara karşı fizik | dönüş çemberi, understeer gradyanı, skidpad sınırı μ·g, yalnızca eğimle tutulan viraj, fren mesafesi, azami hız, vites geçişleri |
| Çarpışma özellik testi | 2.000 rastgele araba-araba çarpışması momentumu korur ve asla enerji yaratmaz |

Görseller üretilmiştir (Higgsfield) ve tüm markalar hayalidir. Motor sesi ve müzik senin ekleyeceğin CC0 kayıtlardır
([liste](assets/src/race/audio/README.md)). O zamana kadar motor sesi sentezlenir.

### `games/smash`: kuleye karşı bir sapan (telefon)

Dik tutulan telefon için, Smash Fest ailesinden bir üst üste dizip devirme oyunu. Sapanı aşağı çekip nişan al,
bırak ve kuleyi masadan düşür: kutular saçılır, tahta ve taş devrilir, cam kavanozlar kırılır. Sekiz bölüm, her
birinde birkaç taş, artan taşlara göre yıldız.

<p align="center">
  <img src="docs/media/smash_shot.jpg" alt="SMASH: kesik çizgiyle nişan, uçan taş, darbe, yıkılış" width="100%">
</p>
<sub>FORTRESS bölümünde tek atış: kesik çizgi taşın ilk değdiği yere tam düşer; ardından hit-stop, enkaz ve yıkılış.
<code>simcraft-smash shot</code> ile pencere açmadan çizildi.</sub>

```bash
tools/mobile/build.sh ios phone        # eşleşmiş iPhone'una Wi-Fi üzerinden derle, kur, başlat
cargo run --release -p sim-mobile --bin simcraft-mobile              # ya da masaüstünde telefon boyutunda pencere
```

#### Nasıl yapıldı

- **Bölümler veridir.** [`games/smash/smash.ron`](games/smash/smash.ron) hissi belirleyen her sayıyı (taş hızı,
  çekme mesafesi, hit-stop, sarsıntı, kamera) ve bölümleri alttan yukarı parça satırları olarak tutar:
  `ccccc / cccc / ccc` bir kutu piramidi, `=` ve `#` tahta ve taş kirişlerdir. Yeni bir bölüm birkaç satırdır.

  <img src="docs/media/smash_levels.jpg" alt="dört bölüm: kutu piramidi, cam raf, sütunlar ve kirişler, cam kule" width="100%">

- **Gerisini fizik motoru yapar.** Kule uyur halde doğar, ilk darbeye kadar kıpırdamadan durur; sonra her parça bir
  katı cisimdir. Cam, bir darbe hızını eşiğinin ötesinde değiştirince kırılır; parçaları yalnızca yere değer.
- **Nişan doğrudandır.** Çekiş kulede bir yükseklik seçer (tabandan tepeye eşit adımlarla), yana çekiş kulenin
  üstünde bir nokta; fırlatma açısı hesaplanır. Bırakınca, parmak kalkmadan 80 ms önceki nişanla atılır.
- **Kameranın derinliği var.** Nişan almak ve telefonu eğmek kamerayı kaydırır ama kamera kuleye bakmaya devam
  eder: kule parmağının altında sabit kalır, arkada kumsal kayar.
- **Geri bildirim büyük anları işaretler:** hit-stop, büyük yıkılışta ağır çekim, travma tabanlı sarsıntı, enkaz,
  titreşim.

#### Adım adım ölçüldü

SMASH eval güdümlü ayarlanır: `simcraft-smash eval` betikli atışları, bir parmağın yaptığı çağrıların aynısıyla taze
kulelerde oynar ve her değişikliği bir öncekiyle karşılaştırır. Kötüleşen adımlar ve reddedilen tahminler dahil her
adım [`games/smash/EVALS.md`](games/smash/EVALS.md)'de.

| Oyuncunun hissettiği | Önce → sonra ölçülen |
|---|---|
| "Çektiğim şeyi göremiyorum" | nişan alırken ekranda görünen cep: zamanın %0'ı → %100'ü |
| "Kulenin bir kısmını vuramıyorum" | parmağın ulaşabildiği kule payı: %64 → %100; vuruş eşit tırmanıyor (16.8× → 1.13× eşitsiz) |
| "Bırakınca atış kayıyor" | bırakırken parmak kayması: 4.3 → 0 cm |
| Kamera sarsılıyor | kamera sarsıntısı (jerk): 16.540 → 635 m/s³ |
| Telefon durmadan titriyor | herhangi bir saniyedeki titreşim: 27 → 9 |
| Kesik çizgi yalan söylüyor | çizgi ile gerçek ilk temas arası: 0.2–0.3 m → yaklaşık 6 cm (bir çözücü hatası buldu: hayalet çarpışmalar) |
| iPhone 14 Pro'da | her yıkılışta 120 fps; 8.3 ms'lik karenin en fazla 4.4 ms'i |

## Başlangıç

[Rust](https://rustup.rs) gerekir (betikli oyuncular ve araçlar için Python 3).

```bash
git clone https://github.com/cemphlvn/simcraft && cd simcraft
cargo build --release -p sim-gpu -p sim-agent
target/release/simcraft-play games/race                                    # sür
echo '{"cmd":"step","n":600}' | target/release/simcraft-agent games/race   # aynı yarış, pencere olmadan
tools/check.sh --quick                                                     # biçim, clippy, testler, tüm oyunlar
```

Telefonunda (iOS için Xcode 26 ve `xcodegen`; Android için JDK 17, Android SDK ve NDK, `cargo-ndk`):

```bash
tools/mobile/build.sh ios phone                                 # eşleşmiş iPhone'un, Wi-Fi üzerinden (kablosuz)
tools/mobile/build.sh ios sim                                   # iOS simülatörü, ekran görüntüsüyle
tools/mobile/build.sh android apk                               # cihaz ya da emülatör için APK
SIMCRAFT_GAMES=games/<name> tools/mobile/build.sh ios phone     # bir kural oyununun bölümlerini oynatıcıya paketle
```

Mobil oyun da veridir: SMASH'in bölümleri ve hissi [`games/smash/smash.ron`](games/smash/smash.ron)'dadır ve
telefon oynatıcısı onu içine gömer. `simcraft-smash check` her düzenlemeden sonra onu okur; `simcraft-smash eval --set
sling.max_speed=28` bir değişikliği dosyaya dokunmadan dener.

Yeni bir oyun: `skills/install.sh`, sonra yapay zekâna *"… olan bir oyun yap"* de.

## Mimari

| Crate | Ne yapar | Oyunu bilir mi? |
|---|---|---|
| `sim-physics` | Sabit nokta sayısal temel, araç dinamiği ve aktarma, 3D katı cisimler, temas, pistler, sürücü yapay zekâsı, ölçüm sahneleri | Hayır (dünyayı bile) |
| `sim-core` | Dünya (varlıklar, ızgara, sürekli hareket, araçlar), efektler, tick döngüsü, hash, anlık görüntüler | Hayır |
| `sim-state` | Durum şemaları: iç içe, katmanlar, yeniden kullanılabilir makineler, remember, interrupt/back, pick | Hayır |
| `sim-rules` | `game.ron` ve `engine.toml`'u yükler, kuralları derler (yerel alt küme + Rhai), kuru çalıştırır ve doğrular | Şemayı, içeriği değil |
| `sim-agent` | `simcraft-agent`: JSON satır protokolü, tekrarlar, gözlem | Hayır |
| `sim-ffi` | Host motorlar için C kütüphanesi ve başlığı | Hayır |
| `sim-gpu` | wgpu çizici ve `simcraft-play`: sahne, pist, voksel ve sürüş görünümleri, ses, his sondası; `simcraft-check` | Hayır |
| `sim-render` | Terminal çizici ve `simcraft-view` | Hayır |
| `sim-mobile` | Telefon: iOS ve Android için tek kabuk, dokunma, titreşim, sensörler, ekran katmanları, 3D, oynatıcı, SMASH ve `simcraft-smash` | Hayır (SMASH kendi kartı) |
| `kernel` | ONNX çizgeleri için tam sayı tensör makinesi (öğrenen ajanlar) | Hayır |
| `test` (`simtest`) | Senaryo dosyaları, altın hash'ler, snapshot'lar, özellik testleri | Hayır |

Tek doğruluk kaynağı [`docs/architecture.md`](docs/architecture.md). Her özelliğin bir oyundan nasıl doğduğu:
[`docs/emergence.md`](docs/emergence.md).

## Kimler için

**Oyun tasarımcıları.** `game.ron`, `engine.toml` ve veri dosyalarıyla çalışırsın; Rust gerekmez. Oyuncunun ne
hissetmesini ve neye karar vermesini istediğini anlatırsın, kuralları yapay zekân yazar, motor çalıştıramadığını
nedeniyle birlikte reddeder. Her değişiklik sabit tohumlarla ölçülebilir.
*Motoru nasıl geliştirirsin:* onu zorlayan oyunlar tasarla. Bir oyun duvara çarptığında (ifade edemediğin bir kural,
ayarlayamadığın bir his) o duvar bir sonraki motor özelliği olur ve kanıtıyla `docs/emergence.md`'ye yazılır.
simcraft'taki özelliklerin çoğu böyle başladı.

**Oyun geliştiricileri.** Test ve botlar için pencere olmadan çalışan deterministik bir çekirdek, Unity ve Unreal
için bir C API, veri olarak kamera, girdi, ses ve glTF modelli bir GPU oynatıcı, ve bir hatayı birebir yeniden
üreten tekrarlar alırsın.
*Motoru nasıl geliştirirsin:* görünümler, kontroller, kameralar ve host adaptörleri yaz (`sim-gpu`, `adapters/`);
görsel getir; oyunları oyna ve yanlış hissettireni bir `--feel` ölçümüyle ya da bir tekrar dosyasıyla göster.

**Mühendisler.** Okunacak kadar küçük bir tam sayı fizik motoru, bir kural derleyicisi, durum şemaları ve bir ölçüm
kültürü alırsın. Her optimizasyon, dünyanın hash'i değişmeden kaydedilen bir adımdır
([`games/traffic/PERF.md`](games/traffic/PERF.md): 1.600 arabada adım adım 94× hız).
*Motoru nasıl geliştirirsin:* fizik katmanının sonraki adımları
[`docs/plans/physics-and-vehicles.md`](docs/plans/physics-and-vehicles.md)'de planlı: alt adımlı yaylarla
süspansiyon, katı cisim çözücüsünün sabit noktaya geçişi, eklemler, dizi-yapısı (SoA) durum. Önce testler gelir, `tools/check.sh`
yeşil kalmalı. Bkz. [`CONTRIBUTING.tr.md`](CONTRIBUTING.tr.md).

## Performans

Apple Silicon dizüstünde, release derlemeler:

| | |
|---|---|
| Araç fiziği, 8 alt adımlı tam model | araba başına tick'te ~150 ns (1.000 araba: 0.15 ms) |
| `games/traffic`, kurallar ve sorgularla 1.600 araba | tick başına 2.72 ms (257 ms'den, [PERF.md](games/traffic/PERF.md)) |
| `games/race` sürüş görünümü, 8 araba + ayna, 1440×810 | kare başına ~5 ms GPU |
| Otopilot turu, pencere olmadan | gerçek zamandan ~13.000× hızlı |
| iPhone 14 Pro'da SMASH | her yıkılışta 120 fps; 8.3 ms'lik karede en fazla 4.4 ms iş |
| Katı cisimler, telefonda yıkılan bir kule | 57 cisim uyanıkken tick başına ≤ 1.4 ms; kule uyurken ~25 µs |

## Desteklenen platformlar

- **Simülasyon çekirdeği** (`sim-core`, `sim-state`, `sim-rules`, `sim-physics`): yerel ve `wasm32`.
- **`simcraft-play`:** macOS'ta (Metal) geliştirildi ve test edildi. wgpu Vulkan ve DX12'yi de hedefler; Linux ve
  Windows derlemeleri planlı.
- **Telefonlar** (`sim-mobile`): iOS; iPhone 14 Pro'da ve simülatörde oynandı, App Store arşivi çıkar. Android APK
  ve Play paketi (16 KB sayfalar) derler, emülatörde çalışır. Android'de titreşim sırada.
- **Host adaptörleri:** C API, C++ sarmalayıcı ve C# `Simulation` test edildi. Unity `SimcraftWorld` bileşeni ve
  Unreal modülü yazıldı, henüz editörlerinde derlenmedi.

## Durum

simcraft genç; 1.0'dan önce kırıcı değişiklikler olacak. Altın hash'ler bunların mevcut oyunlara asla sessizce
olmamasını sağlar. Fizik motorunda araçlar, temas, pistler ve üst üste duran, uyuyan, devrilen 3D katı cisimler
(şimdilik `f32`) var; henüz süspansiyon, eklemler, ragdoll ve tam sürekli çarpışma algılama yok (hızlı mermileri
spekülatif temaslar karşılar).

## Lisans

MIT, bkz. [LICENSE](LICENSE).
