# Generated sources

Made with the Higgsfield CLI (`nano_banana_pro`, 1k, 2 credits each) on 2026-09-28, then turned into pixel art with
`simcraft-pixelate` (key out magenta, crop, scale, reduce colours). The large source PNGs are not committed
(`.gitignore`); regenerate them with the prompts below, or use the job ids.

Style suffix on every backdrop prompt: *16-bit pixel art game background layer, side-scrolling platformer style,
crisp pixels, limited palette, no text, no characters, the layer is isolated on a completely flat solid magenta
#FF00FF background, nothing but magenta above and around it.*

| Asset | Prompt | Aspect | Job | Pixelate |
|---|---|---|---|---|
| `far_mountains` | Distant blue-grey mountain range silhouette with gentle snowless peaks spanning the full width, lighter toward the top, bottom edge flat and filling the lower third | 21:9 | 3bfef15b-b5a5-45ba-89ef-f61502c2344b | `--height 26 --colors 5` |
| `hills` | Rolling green grassy hills spanning the full width, soft shading, a few small shrubs, bottom edge flat and filling the lower half | 21:9 | bac8600e-676b-40d8-a056-3df6455d8b23 | `--height 16 --colors 8` |
| `treeline` | A row of leafy deciduous trees and bushes spanning the full width, varied heights, dark green with light highlights, trunks visible, bottom edge flat | 21:9 | 1f4d31ee-94b1-4f4f-8dcb-0c0aa193dc83 | `--height 14 --colors 8` |
| `soil` | Seamless tileable 16-bit pixel art texture of underground brown soil with small pebbles, grains and tiny roots, top-down flat texture, crisp pixels, limited earthy palette, no border (no style suffix, no key) | 1:1 | bbdc9312-b8d1-441c-b91d-1ae03e9901e4 | `--height 32 --colors 8 --no-key` |

```bash
cargo run --release -p sim-render --bin simcraft-pixelate -- assets/src/hills.png assets/pixel/hills.png --height 18 --colors 8
```
