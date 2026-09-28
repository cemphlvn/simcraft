<div align="center">

# simcraft

**Tasarlayan ve geliştiren sensin. Uygulayan yapay zekân. simcraft ikinizi de dürüst tutar.**

Yapay zekâyla oyun yapanlar için deterministik bir simülasyon motoru.<br>
Okunabilir tek bir oyun dosyası; çalışmadan önce denetlenir, terminalde, Unity'de ve Unreal'da aynı çalışmak için tasarlandı.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org)
[![Agent Skills](https://img.shields.io/badge/AI-agent%20skills-8A2BE2.svg)](skills/)

[Hızlı başlangıç](#hızlı-başlangıç) · [Yapay zekânla yap](#yapay-zekânla-oyun-yap) · [Oyunlar](games/) · [Mimari](docs/architecture.md) · [Katkı](CONTRIBUTING.tr.md) · [English](README.md)

</div>

```
##################
#f.f..........q.q#
#.V......$.....V.#
#f.f..........q.q#
##################
Day 14/150   score  you 124  ·  bob(builder) 131
  YOU (A)  wood  14  stone   0  gold   84  houses 1
  bob (B)  wood   0  stone   3  gold   51  houses 2
  MARKET   wood $18 (stock 3)   stone $10 (stock 14)
  · bob: sell_stone 4
  · house you
> sw 3
```
<sub>`games/market`, terminalde oynanırken: harita, fiyatlar ve kurallar tek bir `game.ron`'dan gelir; bob script'li bir bot.</sub>

## Neden simcraft

- **Tasarımcı sen kalırsın.** Oyun, okuyup inceleyebileceğin ve sürümleyebileceğin tek bir metin dosyası: kind'lar, durumlar, kurallar, oyuncuların yapabilecekleri. Yapay zekân yazar; ne dediğine sen karar verirsin.
- **Hatalar kapıda durur.** Bir kuraldaki, durumdaki ya da parametredeki yazım hatası motoru yükleme anında durdurur; bütün hatalar tek seferde, yapay zekânın düzeltebileceği sözlerle listelenir.
- **Aynı seed, aynı oyun.** Tam sayı aritmetiği, paylaşılan rastgelelik yok: her koşu tick tick tekrar oynatılıp doğrulanabilir, bu yüzden deneyler kanıttır.
- **Görebildiğin davranış.** İç içe durumlar, katmanlar, hafıza ve kesmelerle durum makineleri; Unity Animator'ın ve Unreal StateTree'nin diliyle.
- **Host gösterir, çekirdek karar verir.** Aynı `game.ron` deneyler için arayüzsüz, Unity ya da Unreal içinde çalışır.

## Hızlı başlangıç

[Rust](https://rustup.rs) ve Python 3 gerekir.

```bash
git clone https://github.com/cemphlvn/simcraft && cd simcraft
cargo build --release -p sim-agent
python3 agents/play_market.py        # bir bota karşı 150 gün boyunca oduna taş takas et
```

Sonra `games/market/engine.toml`'da (ya da `game.ron`'daki bir `params` değerinde) tek bir sayıyı değiştir, yeniden oyna ve farklı bir oyun gör.

## Yapay zekânla oyun yap

```bash
skills/install.sh          # bu repoda Claude Code için;  her proje için --user
```

Sonra kendi sözlerinle iste: *"… olan bir oyun yap"*.

| Skill | Yapay zekân ne yapar |
|---|---|
| [`simcraft-game`](skills/simcraft-game/SKILL.md) | Seni tasarımcı olarak görür: oyuncunun ne hissedip neye karar vermesi gerektiğini sorar, `game.ron`'u yazar, motorun denetimini çalıştırır, her hatayı düzeltir, oyunu oynatır ve ne olduğunu anlatır |
| [`simcraft-experiment`](skills/simcraft-experiment/SKILL.md) | *"… olursa ne olur?"* sorusunu yanıtlar: oyunu farklı ayarlar ve seed'lerle defalarca çalıştırır, kanıtı gösterir |
| [`simcraft-eval`](skills/simcraft-eval/SKILL.md) | Eval odaklı geliştirme: "daha iyi"nin ne demek olduğunu sen tanımlarsın, her değişiklik sabit seed'lerle ölçülen tek bir adımdır, öğrendiklerin bir günlükte kalır ([`docs/evals.md`](docs/evals.md)) |

Başka yapay zekâ araçları: onları `skills/<ad>/SKILL.md`'ye (Agent Skills biçimi) ve [`docs/architecture.md`](docs/architecture.md)'ye yönlendir. Yeni skill: `skills/new.sh <ad>`.

## Yolunu seç

| Okuduğun / sevdiğin | Buradan başla | Dokunacağın yer |
|---|---|---|
| **Oyun tasarımı** | [`games/`](games/) (en küçüğü `wolf_sheep`), yukarıdaki skill'ler | `game.ron`, `engine.toml`: kurallar, durum makineleri, denge. Rust gerekmez |
| **Bilgisayar mühendisliği** | [`docs/architecture.md`](docs/architecture.md), sonra [`CONTRIBUTING.tr.md`](CONTRIBUTING.tr.md) | Rust çekirdeği: kural derleyicisi, durum şemaları, determinizm, C API |
| **Sanat ve tasarım** | [`adapters/unity`](adapters/unity/com.simcraft.core), [`adapters/unreal`](adapters/unreal/Simcraft) | Her kind için prefab ve actor, her durum için bir görünüm (`glyphs`): çekirdek karar verirken oyuncunun gördüğü |

**Şimdiye kadar dokuz oyun:** `wolf_sheep` (avcı ve av), `forest_fire` (her boyda yangın), `mercy_dungeon` (dövüş ya da bağışla),
`market` (takas eden iki oyuncu), `gamedev` (kendi motoru üzerinde oyun geliştiren bir stüdyo), `colony` (karıncalar,
koku izleri ve kış; [eval odaklı](games/colony/EVALS.md) yapıldı), `colony3d` (aynı koloni yeraltında: fiziksel bir
yuva, alan olarak sıcaklık ve koku), `forage` (sadece bir beyinle doğan karıncalar doğal seçilimle yiyecek toplamayı
öğrenir; [eval odaklı](games/forage/EVALS.md)), `lanes` (arabada birinci şahıs: 1 2 3 4 yol boyunca pozisyonlar,
boşluk zıplar, Enter atılır; pozisyonlar arası geçişin hissi veriyle ayarlanır). Motorun bu oyunlardan
adım adım nasıl büyüdüğü: [`docs/emergence.md`](docs/emergence.md).

## Tur

### İki dosya

```
games/wolf_sheep/
├── game.ron      # dünya: kind'lar, durumlar, kurallar, oyuncuların yapabilecekleri   (tasarımcının dosyası)
└── engine.toml   # panel: seed, nüfus, şalterler, parametreler                         (operatörün dosyası)
```

### Motorla konuş

```bash
cargo run -q -p sim-agent            # kurt/koyun; başka bir oyun için yol ekle: -- games/market
```

stdin üzerinden, satır başına bir JSON ile konuşulur:

```json
{"cmd":"info"}
{"cmd":"observe"}
{"cmd":"act","actions":[{"entity":41,"do":"move","args":{"dx":1,"dy":1}}]}
{"cmd":"step","n":10}
```

Her satıra bir JSON satırıyla cevap verir. İnsanlar, script'ler ve yapay zekâ agent'ları aynı şekilde oynar.
Script'li oyuncular `agents/` altında (ör. `python3 agents/market.py speculator builder`).
Pazarı bir bota karşı kendin oyna: `python3 agents/play_market.py`.

### Bir kural

```ron
(name: "predation", for: "wolf", when: "near.sheep <= 1",
 then: [ Despawn(Nearest("sheep")), Set("hunger", "0"), Emit("kill") ]),
```

Bir kural hazır eylemlere sığmıyorsa onu [Rhai](https://rhai.rs) ile yaz.

### Bir durum makinesi

Durum içinde durum, yan yana katmanlar, yeniden kullanılan makineler, hatırlama, kesip geri dönme, seçim:

```ron
"Work": (
    remember: true, recheck: true,
    pick: First([ ("Commute", "me.x != me.hx"), ("Build", r#"near_in("project", "Production") == 0"#) ]),
    states: { "Commute": (...), "Build": (use: "focus", rules: [ ... ]) },
),
```

Kurallar durumlara kalıtımla (`state: "Work"`), bileşimle (durumun içine yazılan kurallar) ve uzaklıkla
(`depth`, `steps_to("Shipped")`, `around("dev", "Burnout", 6)`) bağlanır.

### Bir şalter

```toml
[switches]
predation = false
```

### İzle ve tekrar oynat

```toml
[bus]
log = "runs/market.jsonl"          # her eylem, olay ve tick hash'i
listen = "127.0.0.1:7878"          # aynısı, canlı: nc 127.0.0.1 7878
```

```bash
cargo run -q -p sim-agent -- games/market --replay runs/market.jsonl
# {"ok":true,"verified_ticks":150,"acts":234,...}
```

### Gör

```bash
cargo run --release -p sim-render -- games/colony3d     # yüzey, yuva kesiti, 3B; tab bir karıncayı seçer
```

Paralaks tepeler ve mevsimlerle piksel sanatı bir karınca çiftliği (Ghostty, kitty ve WezTerm'de gerçek pikseller; başka
yerlerde yarım bloklar): `cargo run --release -p sim-render -- games/colony3d --view games/colony3d/views/diorama.ron`
(prosedürel arka planlar) ya da `views/generated.ron` (Higgsfield ile üretilip `simcraft-pixelate` ile piksellenmiş).

Tanımladığın ve tıklayarak geçtiğin perspektif durumlarıyla katmanlı 2.5B:
`cargo run --release -p sim-render -- games/colony3d --view games/colony3d/views/layers.ron`.

Zindanı kendin oyna: `cargo run --release -p sim-render -- games/mercy_dungeon` (WASD kahramanı yürütür, F dövüşür,
R bağışlar; IJKL için `--scheme left_hand`). Kontroller de veridir (`games/<ad>/input.ron`): iki el için şemalar ve oyunu
takip eden bağlamlar; aynı tuşlar, neyin seçili olduğuna göre kahramanı yürütür ya da görüntüyü kaydırır.

Arayüz de veridir: `games/colony3d/view.ron` bileşenleri (2B, 2.5B, 3B ya da herhangi bir kesitte dünya görünümleri,
inceleyici, eğilimler) bir tema ve bir asset paketiyle (`assets/ants.ron`) yerleştirir.

## Kaydet ve yükle

`{"cmd":"snapshot"}` oyunun tamamını döndürür; `{"cmd":"restore",...}` o ana geri götürür ve gelecek bit bit aynıdır.

### Unity ve Unreal

Çekirdek aynı zamanda bir C kütüphanesi (`libsimcraft`, başlık `crates/sim-ffi/include/simcraft.h`).
`adapters/build-native.sh` onu host adaptörleri için derler:

- Unity: `adapters/unity/com.simcraft.core` (UPM paketi, `SimcraftWorld` bileşeni)
- Unreal: `adapters/unreal/Simcraft` (plugin, `ASimcraftWorld` actor'ü, Blueprint'ten çağrılabilir)

Host gösterir; çekirdek karar verir. Aynı `game.ron` her yerde çalışır.

## Test et

Bir oyun, oynanarak test edilir, bir dosyada (`test/scenarios/*.ron`): adım at, oyuncu gibi davran, dünyadan bir şey
bekle, gördüğünün snapshot'ını al. Rust gerekmez; motor geliştiricileri aynı kütüphaneyi (`test/`, `simtest` crate'i)
[insta](https://github.com/mitsuhiko/insta) snapshot'ları ve [proptest](https://github.com/proptest-rs/proptest)
özellikleriyle kullanır.

```ron
Scenario(
    name: "wolf_sheep: predation off means no kills",
    game: "games/wolf_sheep",
    switches: { "predation": false },
    steps: [ Step(150), Expect("events.kill == 0") ],
)
```

```bash
cargo run -p simtest          # bütün senaryolar, raporla
cargo test                    # hepsi, snapshot'lar ve özellikler dahil
```

## Durum

simcraft genç. Bugün ne çalışıyor, ne henüz çalışmıyor:

- **Dünyalar ızgaradır**; her konum bir hücre, prop'lar tam sayı. Fizik yok, sürekli uzay yok.
- **Kendi çizicisi yok.** Terminal ASCII gösterir; görüntü bir host'tan gelir (Unity, Unreal).
- **Host adaptörleri:** C API, C++ sarmalayıcı ve C# `Simulation` test edildi; Unity `SimcraftWorld` bileşeni ve
  Unreal modülü yazıldı ama henüz kendi editörlerinde derlenmedi.
- **1.0'dan önce kırıcı değişiklikler olacak.** Golden hash'ler bunların mevcut oyunlarda asla sessizce olmamasını sağlar.

Ayrıntı: [`docs/architecture.md`](docs/architecture.md). Motoru değiştirmek mi istiyorsun? [`CONTRIBUTING.tr.md`](CONTRIBUTING.tr.md).

## Lisans

MIT, bkz. [LICENSE](LICENSE).
