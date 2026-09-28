# simcraft

**Tasarlayan ve geliştiren sensin. Uygulayan yapay zekân. simcraft ikinizi de dürüst tutar.**

simcraft, yapay zekâyla oyun yapanlar için bir simülasyon motoru. Oyunu sen anlatırsın: dünyada kimler yaşıyor,
ne istiyorlar, ne yapabiliyorlar, oyun nasıl bitiyor. Yapay zekân bunu tek, okunabilir bir dosyaya, `game.ron`'a
yazar. Motor çalıştırmadan önce her satırı denetler, bütün hataları tek seferde, yapay zekânın düzeltebileceği
sözlerle bildirir, sonra oyunu her seferinde aynı şekilde çalıştırır.

Oyun dosyası senin: okunur, sürümlenir, incelenir. Aynı dosya deneyler için arayüzsüz (headless), Unity'de ve
Unreal'da çalışır.

[English](README.md)

```
games/wolf_sheep/
├── game.ron      # dünya: kind'lar, durumlar, kurallar, oyuncuların yapabilecekleri   (tasarımcının dosyası)
└── engine.toml   # panel: seed, nüfus, şalterler, parametreler                         (operatörün dosyası)
```

## Bir dakikada

[Rust](https://rustup.rs) ve Python 3 gerekir.

```bash
git clone https://github.com/cemphlvn/simcraft && cd simcraft
cargo build --release -p sim-agent
python3 agents/play_market.py        # bir bota karşı 150 gün boyunca oduna taş takas et
```

Sonra `games/market/engine.toml`'da (ya da `game.ron` params'ında) tek bir sayıyı değiştir, yeniden oyna ve
farklı bir oyun gör.

## Yapay zekânla oyun yap

```bash
skills/install.sh          # bu repoda Claude Code için;  her proje için --user
```

Sonra kendi sözlerinle iste: *"… olan bir oyun yap"*. `simcraft-game` skill'i yapay zekânın seni tasarımcı olarak
görmesini sağlar: oyuncunun ne hissedip neye karar vermesi gerektiğini sorar, `game.ron`'u yazar, motorun
denetimini çalıştırır, her hatayı düzeltir, oyunu oynatır ve ne olduğunu sana anlatır. `simcraft-experiment` ise
*"… olursa ne olur?"* sorularını oyunu defalarca çalıştırıp kanıtı göstererek yanıtlar.

Başka yapay zekâ araçları: onları `skills/<ad>/SKILL.md`'ye (Agent Skills biçimi) ve `docs/architecture.md`'ye yönlendir.

## Yolunu seç

| Okuduğun / sevdiğin | Buradan başla | Dokunacağın yer |
|---|---|---|
| **Oyun tasarımı** | `games/` (beş oyun, en küçüğü `wolf_sheep`), yukarıdaki skill'ler | `game.ron`, `engine.toml`: kurallar, durum makineleri, denge. Rust gerekmez |
| **Bilgisayar mühendisliği** | [`docs/architecture.md`](docs/architecture.md), sonra [`CONTRIBUTING.tr.md`](CONTRIBUTING.tr.md) | Rust çekirdeği: kural derleyicisi, durum şemaları, determinizm, C API |
| **Sanat ve tasarım** | `adapters/unity`, `adapters/unreal` | Her kind için prefab ve actor, her durum için bir görünüm (`glyphs`); çekirdek karar verirken oyuncunun gördüğü |

Şimdiye kadar beş oyun var, her biri `games/` altında kendi klasöründe: `wolf_sheep`, `forest_fire`,
`mercy_dungeon`, `market` (iki oyunculu), `gamedev` (kendi motoru üzerinde oyun geliştiren bir stüdyo). Motorun bu
oyunlardan adım adım nasıl büyüdüğü: [`docs/emergence.md`](docs/emergence.md).

## Motorla konuş

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

## Bir kural

```ron
(name: "predation", for: "wolf", when: "near.sheep <= 1",
 then: [ Despawn(Nearest("sheep")), Set("hunger", "0"), Emit("kill") ]),
```

Bir kural hazır eylemlere sığmıyorsa onu [Rhai](https://rhai.rs) ile yaz.

## Bir durum makinesi

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

## Bir şalter

```toml
[switches]
predation = false
```

## İzle ve tekrar oynat

```toml
[bus]
log = "runs/market.jsonl"          # her eylem, olay ve tick hash'i
listen = "127.0.0.1:7878"          # aynısı, canlı: nc 127.0.0.1 7878
```

```bash
cargo run -q -p sim-agent -- games/market --replay runs/market.jsonl
# {"ok":true,"verified_ticks":150,"acts":234,...}
```

## Kaydet ve yükle

`{"cmd":"snapshot"}` oyunun tamamını döndürür; `{"cmd":"restore",...}` o ana geri götürür ve gelecek bit bit aynıdır.

## Unity ve Unreal

Çekirdek aynı zamanda bir C kütüphanesi (`libsimcraft`, başlık `crates/sim-ffi/include/simcraft.h`).
`adapters/build-native.sh` onu host adaptörleri için derler:

- Unity: `adapters/unity/com.simcraft.core` (UPM paketi, `SimcraftWorld` bileşeni)
- Unreal: `adapters/unreal/Simcraft` (plugin, `ASimcraftWorld` actor'ü, Blueprint'ten çağrılabilir)

Host gösterir; çekirdek karar verir. Aynı `game.ron` her yerde çalışır.

## Güvenceler

- **Deterministik.** Aynı seed, aynı girdiler, her tick'te aynı dünya.
- **Çalışmadan önce denetlenir.** Bir kuraldaki, şalterdeki ya da parametredeki yazım hatası motoru yükleme anında durdurur.

Ayrıntı: [`docs/architecture.md`](docs/architecture.md). Motoru değiştirmek mi istiyorsun? [`CONTRIBUTING.tr.md`](CONTRIBUTING.tr.md).

## Lisans

MIT — bkz. [LICENSE](LICENSE).
