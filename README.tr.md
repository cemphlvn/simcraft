<div align="center">

# simcraft

**Kendi fizik motoruyla, masaüstünde ve telefonunda çalışan deterministik bir oyun motoru.**<br>
Oyunlar veri dosyalarıdır: çalışmadan önce denetlenir, bit bit tekrar oynatılır, adım adım ölçülür, yapay zekâyla yapılır.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org)
[![Agent Skills](https://img.shields.io/badge/AI-agent%20skills-8A2BE2.svg)](skills/)

[Başlangıç](#başlangıç) · [Oyunlar](#oyunlar) · [Özellikler](#özellikler) · [Mimari](#mimari) · [Belgeler](docs/architecture.md) · [Katkı](CONTRIBUTING.tr.md) · [English](README.md)

</div>

<p align="center">
  <img src="docs/media/race_chase.jpg" alt="games/race: eğimli bir ovalde sekiz stock car" width="100%">
</p>

## Neden simcraft

- **Deterministik.** Tam sayı aritmetiği, sabit zaman adımı, paylaşılan rastgelelik yok: aynı tohum ve girdi, her
  makinede aynı dünya. Koşular bit bit tekrar oynar. (Bilerek yapılmış tek istisna: 3D katı cisim çözücüsü, ilk
  oyununun hissi oturana kadar `f32`; kural motorunun dışında durur.)
- **Önce veri.** Bir oyun bir `game.ron` ve bir `engine.toml`'dır; araba, pist ya da mobil bölüm de birer dosyadır.
- **Çalışmadan önce denetlenir.** Tüm hatalar yüklemede, nerede oldukları ve nasıl düzeltilecekleriyle listelenir.
- **Ölçülür.** Fizik kapalı form cevaplara, performans kayıtlı ölçümlere karşı; his sondalarla; her değişiklik
  kaydedilmiş bir eval adımı olarak.
- **Masaüstünden telefona.** Telefon uygulaması bir kez derlenir ve oyunları veri olarak oynar: bir değişiklik bir
  dakikadan kısa sürede iPhone'undadır.

## Başlangıç

```bash
git clone https://github.com/cemphlvn/simcraft && cd simcraft
cargo build --release -p sim-gpu -p sim-agent
target/release/simcraft-play games/race      # sür
tools/check.sh --quick                       # biçim, clippy, testler, tüm oyunlar denetlenir
tools/mobile/build.sh ios phone              # SMASH, Wi-Fi üzerinden iPhone'unda (ayrıca: ios sim, android apk)
```

Yapay zekânla yeni bir oyun: `skills/install.sh`, sonra *"… olan bir oyun yap"*. Her oyun stdin üzerinden JSON da
konuşur (`{"cmd":"step","n":10}`): betikler, testler ve ajanlar için.

## Oyunlar

### `games/race`: eğimli ovalde stock car (masaüstü)

Charlotte'un 1.5 millik, 24° eğimli ovalinde sekiz araba; yedisi kendini sürer. Her araba bir teknik föy
([`stock_car.ron`](assets/vehicles/stock_car.ron)), pist düzlükler ve eğimli virajlardan bir dosya
([`charlotte.ron`](games/race/tracks/charlotte.ron)); motorun hiçbir yeri bunun bir yarış olduğunu bilmez. Otopilot
turu 30.75 s, gerçek pole 29.355 s ([kayıt](games/race/LAPS.md)).

### `games/smash`: kuleye karşı bir sapan (telefon)

<p align="center">
  <img src="docs/media/smash_shot.jpg" alt="SMASH: kesik çizgiyle nişan, uçan taş, darbe, yıkılış" width="100%">
</p>

Çekip nişan al, bırak, kuleyi masadan düşür. Sekiz bölüm, her biri [`smash.ron`](games/smash/smash.ron) içinde birkaç
satır veri (`ccccc / cccc / ccc` bir kutu piramidi). Eval güdümlü ayarlandı, her adım
[`EVALS.md`](games/smash/EVALS.md)'de:

| Oyuncunun hissettiği | Önce → sonra |
|---|---|
| "Kulenin bir kısmını vuramıyorum" | ulaşılabilen: %64 → %100 |
| "Bırakınca atış kayıyor" | bırakırken kayma: 4.3 → 0 cm |
| Kamera sarsılıyor | jerk: 16.540 → 635 m/s³ |
| iPhone 14 Pro'da | her yıkılışta 120 fps |

## Özellikler

- **Fizik** (`sim-physics`): sabit nokta sayısal temel; lastikli araç dinamiği, aktarma, ABS ve çekiş kontrolü;
  üst üste duran, uyuyan ve devrilen 3D katı cisimler (Box2D v3 soft step, SAT, spekülatif temaslar); veri olarak
  pistler; yarış yapay zekâsı.
- **Simülasyon** (`sim-core`, `sim-state`, `sim-rules`): sürekli hareketli ızgara ve voksel dünyalar, durum
  şemaları, veri olarak kurallar (yerel ve [Rhai](https://rhai.rs)), anlık görüntüler ve tekrarlar.
- **Görüntü** (`sim-gpu`, `sim-render`): wgpu görünümleri (sahne, pist, voksel, kokpit), veri olarak kamera, girdi
  ve ses, terminal çizici.
- **Mobil** (`sim-mobile`): iPhone ve Android için tek kabuk, dokunma hareketleri, titreşim, eğim, güvenli alan
  katmanları, gölgeli 3D.
- **Araçlar:** JSON protokolü ve Unity, Unreal için C API; eval'lar (`tools/eval.py`, `simcraft-smash eval`), hash
  kontrollü ölçümler, pencere açmadan ekran görüntüsü, [yapay zekâ ajan becerileri](skills/).

## Mimari

| Crate | Ne yapar |
|---|---|
| `sim-physics` | Sayısal temel, araçlar, katı cisimler, temas, pistler (başka hiçbir şeye bağlı değil) |
| `sim-core` | Dünya, tick döngüsü, efektler, hash, anlık görüntüler |
| `sim-state` / `sim-rules` | Durum şemaları / bir oyunu yükleme, derleme ve denetleme |
| `sim-agent` / `sim-ffi` | JSON protokolü / host motorlar için C API |
| `sim-gpu` / `sim-render` | `simcraft-play` ve GPU görünümleri / terminal çizici |
| `sim-mobile` | Telefon: kabuk, dokunma, titreşim, 3D, oynatıcı ve SMASH |
| `kernel`, `test` | Öğrenen ajanlar için ONNX çizgeleri; senaryolar, altın hash'ler, özellik testleri |

Hiçbiri belirli bir oyunu bilmez. Tek doğruluk kaynağı [`docs/architecture.md`](docs/architecture.md); her
özelliğin bir oyundan nasıl doğduğu [`docs/emergence.md`](docs/emergence.md)'de.

## Performans

| Apple Silicon, release | |
|---|---|
| Araç fiziği, 8 alt adım | araba başına tick'te ~150 ns |
| `games/traffic`, 1.600 araba | tick başına 2.72 ms, 257 ms'den ([PERF.md](games/traffic/PERF.md)) |
| iPhone 14 Pro'da SMASH | 8.3 ms'lik karede en fazla 4.4 ms iş |

## Durum

Genç: 1.0'dan önce kırıcı değişiklikler olacak, ama altın hash'ler bunların mevcut bir oyunu asla sessizce
değiştirmemesini sağlar. macOS, iOS ve Android'de çalışır (Linux ve Windows planlı); Unity ve Unreal adaptörleri
yazıldı ama henüz editörlerinde derlenmedi. Katkı için [`CONTRIBUTING.tr.md`](CONTRIBUTING.tr.md).

MIT lisanslı, bkz. [LICENSE](LICENSE).
