# simcraft'a katkı

[English](CONTRIBUTING.md)

Tasarlayan ve geliştiren sensin; uygulamayı yapay zekân yapabilir. Katkılar için de bu geçerli: istediğin yapay zekâ
aracını kullan, ama tasarımın ve incelemenin sahibi **sensin**. Pull request'indeki bir satırı açıklayamıyorsan,
o PR henüz hazır değil.

## Dört katkı yolu

| Katkı | Nerede | Rust gerekir mi? |
|---|---|---|
| **Bir oyun** | `games/<ad>/game.ron` + `engine.toml` | Hayır |
| **Bir motor değişikliği** | `crates/` | Evet |
| **Bir host adaptörü ya da görünüm** (Unity, Unreal, sanat) | `adapters/` | Hayır (C#, C++, asset'ler) |
| **Bir skill** (yapay zekânın bir işte nasıl yardım edeceği) | `skills/<ad>/SKILL.md` | Hayır |

## Bir oyun

Bir tasarımcının okumak isteyeceği gibi yaz: adlandırılmış parametreler, açık olmayan her kurala bir yorum,
tasarımcının kelimeleriyle adlandırılmış durumlar. Oyun yüklenmeli
(`echo '{"cmd":"quit"}' | cargo run -q -p sim-agent -- games/<ad>`) ve `error:` olayı üretmeden çalışmalı. `test/scenarios/` altına oyunu oynayıp önemli olanı bekleyen bir senaryo ekle
(`cargo run -p simtest -- test/scenarios/<ad>.ron`; biçim `docs/architecture.md`, "Testing"). İlginç bir
şey bulduysan (kazanan bir strateji, bir devrilme noktası) sayılarıyla birlikte `docs/emergence.md`'ye ekle.

## Bir motor değişikliği

simcraft önceden tasarlanmaz: **motor, bir oyun duvara çarptığında değişir.** Önce oyunu, keşke böyle
yazabilseydim dediğin şekilde yaz; yüklenemediği ya da bir şeyi ifade edemediği yer, değişikliğin gerekçesidir.

1. [`docs/emergence.md`](docs/emergence.md)'ye kaydet: belirti → ihtiyaç → değişiklik → kanıt.
2. Protokol ya da kural dili değişiyorsa **önce** [`docs/architecture.md`](docs/architecture.md)'yi güncelle.
   Tek doğruluk kaynağı odur.
3. Çekirdeğin kurallarına uy ([`AGENTS.md`](AGENTS.md), yapay zekâ aracın da onu okur):
   - `sim-core` hiçbir oyunu bilmez.
   - Determinizm: `HashMap` üzerinde gezinme yok, float yok, paylaşılan RNG yok, script'lerden I/O yok.
   - Mevcut oyunlar sessizce değişmemeli: golden hash'ler onları korur. Bir hash değişirse PR'da nedenini yaz.
4. PR açmadan önce:
   ```bash
   cargo test && cargo clippy --all-targets
   ```

## Bir host adaptörü

Çekirdek karar verir; host yalnızca gösterir. Adaptör kodu C API ile (`crates/sim-ffi/include/simcraft.h`)
sarmalayıcılar üzerinden konuşur (C#'ta `Simulation`, C++'ta `simcraft.hpp`). Native kütüphaneyi
`adapters/build-native.sh` ile derle. Unity sarmalayıcısının düz .NET testleri var:
`dotnet run --project adapters/unity/tests`. PR'da neyi hangi editör sürümünde derleyip çalıştırdığını yaz.

## Bir skill

```bash
skills/new.sh simcraft-<konu>      # skills/_template'ten
skills/install.sh                  # Claude Code'da dene
```

Bir skill, yapay zekâya bir insana tek bir işte nasıl yardım edeceğini anlatır. Rolleri net tut (insan tasarlar ve
karar verir, yapay zekâ uygular ve raporlar) ve **repoyu kopyalamak yerine ona işaret et**: `docs/architecture.md`'ye,
örnek oyunlara ve script'lere bağlantı ver ki skill motordan kopmasın.

## Commit mesajları

Commit mesajı, bir sonraki kişiye onun için neyin değiştiğini söyler. Kodu yazan için değil, oyun yapan biri için
yaz.

**Başlık satırı**

- Neyin değiştiğini düz sözlerle söyle: bir oyuncu, tasarımcı ya da geliştirici artık ne yapabilir, ne görür.
  Yaklaşık 72 karakter, sonda nokta yok.
- Değişikliğin kendisini anlat, nasıl ortaya çıktığını değil: "istendiği gibi", "araştırmaya göre",
  "… tarzında", "review düzeltmeleri" yok.
- İşe yarıyorsa bir alan adı ver: `README: …`, `Race: …`, `Drive view: …`.

**Gövde**

- Maddeler; her biri somut bir değişiklik, diff'i okuyacak birinin sırasıyla.
- Bir tekniği yerleşik adıyla an ki okuyan arayıp bulabilsin: "sweep and prune", "sequential impulses",
  "dirty flag (Game Programming Patterns)".
- Ölçülen sayıları öncesi → sonrası diye yaz: `2.000 rastgele çarpışma momentumu korur`,
  `1.600 arabada 257 → 2.72 ms`.
- Denenip bırakılanı, bir şey öğretiyorsa tut ("gradyanı düzeltmelere de katmak 75 m/s'de şerit değişimini
  sallantıya soktu").
- Araç ya da asistan atıf satırları ekleme.

**Commit başına tek değişiklik.** Her commit tek başına derlenmeli ve `tools/check.sh --quick`'ten geçmeli.
`git fetch`'ten sonra push et (başkaları da `main`'e push ediyor), en eski commit önce. Kendine not gibi okunan bir
mesajı push etmeden önce yeniden yaz.

| Bunun yerine | Bunu yaz |
|---|---|
| `Fix stuff in drive.rs` | `Race: line steering reads its own stick axis (it crawled at 0.36 m/s at speed)` |
| `Implement feature per plan step 3` | `Drivetrain: the engine turns through a sequential gearbox, and RPM and gear reach the game` |

(Commit mesajları, kod gibi, İngilizce yazılır.)

## Pull request'ler

- Her PR'da tek fikir, tek oturumda incelenebilecek kadar küçük.
- Kanıtı ekle: bir test, bir hash, bir tablo, bir ekran görüntüsü.
- Kod, yorumlar ve dokümanlar İngilizce; README ve bu rehberin Türkçesi de var, ikisini birlikte güncel tut.
- İncelemede nazik ol. Buradaki herkes öğreniyor, motor da dahil.
