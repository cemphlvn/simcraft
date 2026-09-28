# Research: what platforms accept, and whether there is a common format

2026-09-28. Question: beyond rendering to a terminal, what do stores and devices accept, and is there a shared
protocol or format across Steam, Epic/Unreal, Unity, Rockstar, iOS, Google Play, WeChat, Apple Vision Pro and
Meta Ray-Ban?

## What each one accepts

| Platform | What you submit | Graphics / runtime | Notes |
|---|---|---|---|
| **Steam** | Native builds per OS, uploaded as **depots** with SteamPipe (build scripts, `steamcmd`) | Whatever the binary uses (DX12/Vulkan/Metal/GL) | macOS `.app` must be signed and notarized; Linux binaries; Windows builds also reach Steam Deck through Proton |
| **Epic Games Store** (Unreal's store) | Native PC/Mac binaries via **BuildPatchTool**, self-publishing through the Dev Portal | Native | $100 recoupable fee; crossplay required for multiplayer across PC stores |
| **Unity Asset Store / Fab** | Not games: **asset packages** (`.unitypackage`, UPM packages; Fab file-format rules) | – | This is where a *simcraft plugin* would be sold, not a game |
| **Rockstar** | **No third-party games.** Creator channel = FiveM/RedM mods (Cfx.re, now part of Rockstar), sold on the Cfx Marketplace | Scripts on top of GTA V / RDR2 | Only relevant as "mods for their games" |
| **iOS App Store** | **IPA** via App Store Connect; from 28 Apr 2026 built with **Xcode 26 / iOS 26 SDK** | Metal (native); WebGPU in Safari 26 | Deployment target can be lower than the SDK |
| **Google Play** | **Android App Bundle (AAB)**; from 31 Aug 2026 target **API 36** (existing apps API 35) | Vulkan / GLES (native); WebGPU in Chrome ≥121 on Android 12+ | |
| **WeChat Mini Games** | A mini-game package: **JS or WebAssembly** on WeChat's **WebGL** wrapper; **4 MB** first package + subpackages | WebGL (not WebGPU) | In-app payments need a Chinese game licence (a local publisher for foreign studios) |
| **Apple Vision Pro** | visionOS app via App Store Connect (same pipeline as iOS) | RealityKit (shared space) or Metal via Compositor Services (immersive); Unity via PolySpatial; **WebXR** in Safari | One WebXR build also runs on Quest and Pico |
| **Meta Ray-Ban Display** | Two paths (developer preview, May 2026): (1) **Wearables Device Access Toolkit**, native iOS (Swift) / Android (Kotlin) SDK extending a phone app to the glasses; (2) **Web Apps** in HTML/CSS/JS running directly on the glasses | Monocular display; web runtime | Web apps get motion/orientation, phone GPS, Neural Band and captouch input, local storage |

## Is there a common protocol or format?

**For submission: no.** Every store wants its own package (depots, BuildPatchTool, IPA, AAB, WeChat package,
visionOS app) and its own signing, review and rating process. That layer stays per platform, and is thin.

**Underneath, there are two common layers:**

1. **The web stack: WebAssembly + WebGL/WebGPU (+ WebXR).** It reaches the web (itch.io, portals), **WeChat**
   (wasm + WebGL), **Meta Ray-Ban Display** (web apps), **Vision Pro and Quest** (WebXR), and every phone browser.
   WebGPU is now in all major browsers (Chrome/Edge since 2023, Safari 26 on macOS/iOS/iPadOS/visionOS, Firefox 141+,
   Chrome on Android 12+); WeChat is still WebGL.
2. **A portable GPU API: `wgpu`** (Rust). One renderer runs on **Metal** (macOS, iOS, visionOS), **Vulkan**
   (Linux, Android, Windows), **DX12** (Windows), **GLES**, and in wasm on **WebGPU or WebGL2**. The same code covers
   the native stores (Steam, Epic, App Store, Play) and the web targets.

Headsets outside Apple share **OpenXR** natively; asset interchange shares **glTF** (and USDZ on Apple).

## What this means for simcraft

- The **core** (deterministic, integer, no I/O) should compile to **wasm32** as well as native: it is the part that
  runs everywhere unchanged, as `game.ron` already does.
- The **renderer** should gain a **GPU backend on `wgpu`**, with the same components, views, assets and
  projections. This is also the real answer to the texture question: cook PNGs at build time into GPU-ready atlases,
  upload once, composite on the GPU. The terminal renderer stays as the developer and debugging view.
- The **store layer** is a thin shell per platform (a Steam depot, an IPA, an AAB, a WeChat package, a web bundle).
- Priority by reach and cost: web (wasm + WebGPU/WebGL2) first, since it also covers WeChat, the Ray-Ban web apps and
  WebXR; then native desktop for Steam and Epic from the same wgpu code; then mobile shells.

Sources: [Steamworks: Uploading to Steam](https://partner.steamgames.com/doc/sdk/uploading) ·
[Steamworks: Platforms](https://partner.steamgames.com/doc/store/application/platforms) ·
[Epic: Upload binaries with BuildPatch Tool](https://dev.epicgames.com/docs/epic-games-store/publishing-tools/uploading-binaries) ·
[Epic Games Store requirements](https://dev.epicgames.com/docs/epic-games-store/requirements-guidelines/distribution-requirements/requirements-overview) ·
[Epic self-publishing launch](https://store.epicgames.com/en-US/news/epic-games-store-launches-self-publishing-tools-for-game-developers-and-publishers) ·
[Unity Asset Store submission guidelines](https://assetstore.unity.com/publishing/submission-guidelines) ·
[Fab asset file format requirements](https://dev.epicgames.com/documentation/en-us/fab/asset-file-format-and-structure-requirements-in-fab) ·
[Rockstar: Roleplay Community Update (Cfx.re)](https://www.rockstargames.com/newswire/article/8971o8789584a4/roleplay-community-update) ·
[Cfx Marketplace](https://wccftech.com/rockstar-launches-cfx-marketplace-official-modding-store-fivem-redm/) ·
[Apple: upcoming SDK minimum requirements](https://developer.apple.com/news/?id=ueeok6yw) ·
[Google Play target API level requirements](https://support.google.com/googleplay/android-developer/answer/11926878?hl=en) ·
[Android App Bundle](https://en.wikipedia.org/wiki/Android_App_Bundle) ·
[WeChat code package size optimization](https://developers.weixin.qq.com/miniprogram/en/dev/framework/performance/tips/start_optimizeA.html) ·
[Game engines for WeChat Mini Games (2026)](https://app.cinevva.com/guides/wechat-mini-game-engines) ·
[Cocos: Publish to WeChat Mini Games](https://docs.cocos.com/creator/3.1/manual/en/editor/publish/publish-wechatgame.html) ·
[Meta: Build for display glasses](https://developers.meta.com/blog/build-for-display-glasses/) ·
[Meta Wearables Device Access Toolkit](https://developers.meta.com/blog/introducing-meta-wearables-device-access-toolkit/) ·
[Road to VR on the toolkit](https://roadtovr.com/meta-ray-ban-smart-glasses-third-party-app-sdk-device-access-toolkit/) ·
[Unity PolySpatial visionOS overview](https://docs.unity3d.com/Packages/com.unity.polyspatial.visionos@2.2/manual/visionOSPlatformOverview.html) ·
[WebGPU supported in major browsers](https://web.dev/blog/webgpu-supported-major-browsers) ·
[WebGPU implementation status](https://github.com/gpuweb/gpuweb/wiki/Implementation-Status) ·
[wgpu](https://github.com/gfx-rs/wgpu)
