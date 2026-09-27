# simcraft: mimari

Kuralları dışarıdan tanımlanan, headless ve deterministik bir simülasyon motoru.
Bu dosya projenin **tek doğruluk kaynağı**. Kod bununla çelişirse ya kod düzeltilir ya da önce bu dosya güncellenir.

## Üç rol, üç dosya

| Rol | Dosya | Neye karar verir | Dokunmadığı şey |
|---|---|---|---|
| **Tasarımcı** | `games/<oyun>/game.ron` | Dünya: kind'lar, FSM'ler, kurallar, ayar düğmeleri (`params`) | Seed, nüfus, hangi kuralın açık olduğu |
| **Yönetici** (buharlı motorun başındaki kişi) | `games/<oyun>/engine.toml` | Motorun nasıl yanacağı: seed, süre, dünya boyutu, başlangıç nüfusu, **şalterler**, **hiperparametreler**, Rhai emniyet ventili, agent erişimi | Kuralların kendisi |
| **Agent** | stdin/stdout JSON | Oyun içindeki kontrol edilebilir entity'lerin hareketi | Kurallar ve panel |

Yönetici yalnızca tasarımcının açtığı düğmeleri çevirebilir. `engine.toml`'da `game.ron`'da olmayan bir kural, parametre ya da kind geçerse motor **çalışmaz**. Yazım hataları sessizce yutulmaz.

## Katmanlar

```
game.ron ──┐                    ┌── sim-agent  (JSON stdio; ilk istemci)
engine.toml┴─► sim-rules ──────►│
              (parse, Rhai       └── (sonra) sim-tui, replay
               derle, validate)
                     │ impl Rules
                     ▼
               sim-core  (World, Effect, apply, Engine<typestate>)
```

| Crate | İçerik | Oyunu bilir mi? |
|---|---|---|
| `sim-core` | `World`, `Effect`, `Group`, `apply`, `Engine<Loaded→Validated→Running>`, `trait Rules`, hash | Hayır |
| `sim-rules` | `GameDef` (RON), `EngineConfig` (TOML), Rhai derleme, dry-run doğrulama, `impl Rules for Game` | Şemayı bilir, içeriği bilmez |
| `sim-agent` | `simcraft-agent` binary'si, JSON satır protokolü, ASCII harita | Hayır |

## Tick döngüsü

```
tick:
  groups = agent kuyruğu            (önce: agent hareketi kural hareketini ezer)
         + FSM geçişleri            (entity başına ilk eşleşen)
         + kurallar                 (entity id sırası × game.ron kural sırası)
  apply(groups)                     (tek yazma noktası)
  tick += 1; hash
```

- **Kurallar dünyayı değiştirmez**, `Group` (bir ateşlemenin effect'leri) üretir.
- **Grup atomiktir:** dokunduğu bir entity bu tick'te daha önce öldüyse grubun tamamı düşer. Ölen başkasıysa `conflict` olayı üretilir (iki kurt aynı koyunu yiyemez), ölen grubun sahibiyse sessizce düşer.
- Bir entity bir tick'te **en fazla bir kez hareket eder**, ilk gelen `Move` kazanır.
- FSM geçişi `apply`'da uygulanır. Kurallar o tick boyunca eski durumu görür.

## Determinizm (pazarlıksız)

| Kaynak | Önlem |
|---|---|
| Iterasyon sırası | `BTreeMap`, id sırası |
| Rastgelelik | Paylaşılan RNG yok: `rand(seed, tick, entity, salt)`, splitmix64. Değerlendirme sırası sonucu etkilemez |
| Aritmetik | Rhai `no_float` + `only_i64`, prop'lar `i64` |
| Script yan etkisi | `me`, `p`, `near` vb. sabit. Script yalnızca effect map'i döndürür, `print` kapalı |
| Doğrulama | Test: aynı panel → 300 tick boyunca her tick'te aynı hash |

## Kural dili

**A. Bildirimsel (varsayılan):** `when` (Rhai ifadesi → bool), ardından `then: [...]`.

| Eylem | Anlamı |
|---|---|
| `Set(prop, expr)` / `Add(prop, expr)` | Kendi prop'unu yazar |
| `Emit(name)` | Olay üretir (agent'lar görür) |
| `Despawn(Me \| Nearest(kind))` | Hedef yoksa kural ateşlenmez |
| `Spawn(kind)` | Kendi konumunda, kind şablonuyla doğar |
| `MoveToward(kind)` / `MoveAway(kind)` / `Wander` | 1 adım (8 yön) |

**B. Rhai script (kaçış kapısı):** `script:` alanı effect map dizisi döndürür:
`#{op: "set"|"add", prop, value}`, `#{op: "emit", name}`, `#{op: "move", dx, dy}`, `#{op: "despawn"}`.

**İfadelerin gördükleri:** `me.<prop>`, `me.x/y/state/kind/id`, `p.<param>`, `near.<kind>` (Chebyshev mesafesi, yoksa 9999), `count.<kind>`, `tick`, `roll` (0..99, kural ve entity başına deterministik).

## Doğrulama (typestate `Loaded → Validated`)

`Engine<Loaded>` üzerinde `tick` metodu yoktur. Doğrulanmamış kural derleme zamanında koşamaz. `validate` tüm hataları tek listede döner:

1. RON/TOML şeması (`deny_unknown_fields`)
2. Rhai sözdizimi (yükleme anında derlenir)
3. Çapraz referanslar: şalter ↔ kural adı, param ↔ `game.ron params`, kind, FSM, state
4. **Dry-run:** her ifade her ilgili kind'ın şablonuyla bir kez koşturulur. `me.hungr` gibi yazım hataları burada yakalanır (`fail_on_invalid_map_property`)

Oyun (runtime) durumları **veridir** (`FSM`, string). Motorun durumları **tiptir** (typestate).

## Agent protokolü (`simcraft-agent [GAME_DIR] [--config PANEL.toml]`)

Satır başına bir JSON istek, satır başına bir JSON cevap. Açılışta `{"ok":true,"ready":...}`, hatada `{"ok":false,"stage":"load|validate","errors":[...]}` basılır ve çıkış kodu 2 olur.

| İstek | Cevap |
|---|---|
| `{"cmd":"info"}` | Oyun, kind'lar (glyph, prop, state), kontrol edilebilir kind'lar, etkin şalterler/parametreler, komut şeması |
| `{"cmd":"observe"}` | Tüm harita (ASCII), sayımlar, entity'ler |
| `{"cmd":"observe","entity":ID}` | `observe_radius` penceresi, `@` = sen, görünen entity'ler |
| `{"cmd":"act","actions":[{"entity":ID,"move":[dx,dy]}]}` | Bir sonraki `step`'te, kurallardan önce uygulanır |
| `{"cmd":"step","n":N}` | tick, done, hash, sayımlar, olaylar (`kill`, `born`, `starved`, `conflict`, `error: …`) |
| `{"cmd":"hash"}` | Durum parmak izi (replay/doğrulama) |

## Yol haritası

- [x] core + typestate + atomik grup + determinizm testleri
- [x] RON (A) + Rhai (B) + dry-run doğrulama
- [x] engine.toml paneli (şalter, hiperparametre, ventil)
- [x] JSON stdio agent arayüzü, kurt/koyun
- [ ] Olay günlüğü → `sim-replay` (aynı log → aynı hash)
- [ ] MCP sarmalayıcı (LOBI / travian-bench agent'ları doğrudan bağlansın)
- [ ] `sim-tui` (ratatui) izleyici
- [ ] Parametre taraması: yöneticinin paneli otomatik ayarlaması (hayatta kalma / salınım skoru)
- [ ] Performans: sıcak prop'ları typed column'a taşımak, `near` için spatial grid (şu an O(n²))
