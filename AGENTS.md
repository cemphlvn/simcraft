# simcraft

Önce `docs/architecture.md` okunur. Mimarinin tek doğruluk kaynağı odur.

- Yeni oyun = `games/<ad>/game.ron` + `engine.toml`. Motor koduna dokunulmaz.
- `sim-core` oyun hakkında hiçbir şey bilmez. Oyuna özgü mantık oraya girmez.
- Determinizm kırılamaz: `HashMap` iterasyonu, float, paylaşılan RNG ve script içinden I/O yasak.
- Değişiklikten sonra: `cargo test && cargo clippy --all-targets`.
- Protokol ya da kural dili değişirse önce `docs/architecture.md` güncellenir.
