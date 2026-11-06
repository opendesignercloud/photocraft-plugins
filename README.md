# PhotoCraft plug-ins

Free, open source plug-ins for [PhotoCraft](https://opendesignercloud.com), from Open Designer Cloud. PhotoCraft plug-ins are single WebAssembly (`.wasm`) files that run in a sandbox, so one file works on macOS, Windows, Linux and the web build.

| Plug-in | What it does | Download |
|---|---|---|
| [ODC Film Look](odc-film-look/) | 11 film looks (Kodachrome, Tri-X, Fuji Superia, bleach bypass and more), grain, vignette, faded blacks, warmth | [Releases](../../releases) |
| [ODC Lens](odc-lens/) | Soft corners, colour fringes, bloom and halation, barrel distortion, vignette, with 6 lens looks | [Releases](../../releases) |
| [ODC Glow](odc-glow/) | Orton effect, glamour glow, soft focus, Pro-Mist haze, dreamy haze, golden glow, with 6 looks | [Releases](../../releases) |
| [ODC Tone](odc-tone/) | Fix the light: Interior / Real Estate, Twilight, Overcast, Backlit, Product on White, Soft Portrait, Clarity Punch, with local contrast, texture and sharpness | [Releases](../../releases) |
| [ODC Mono](odc-mono/) | Black and white like film: stock colour response, colour filters, toning (sepia, selenium, cyanotype, platinum), grain, with 11 looks | [Releases](../../releases) |
| [ODC Retro](odc-retro/) | VHS, CRT scanlines, glitch, and pixel art with fixed palettes and ordered dither (Game Boy, 1-bit, CGA, PICO-8, amber terminal), with 8 looks | [Releases](../../releases) |

## Install a plug-in

1. Download the `.wasm` file from the plug-in's release (or from its `dist/` folder).
2. In PhotoCraft choose **Filter > Plug-ins > Install Plug-in...** and pick the file, or put it in a folder and set **Edit > Preferences > Plug-ins > Additional Plug-ins Folder**.

Each plug-in folder has its own README, license, build script and tests. To build one yourself you need Rust with the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`), then run `./build.sh` in the plug-in's folder.

## Write your own

The plug-in interface (ABI v1) is documented in PhotoCraft's `docs/plugins.md`. ODC Film Look is a complete, commented example: `no_std` Rust, no dependencies, a manifest with parameters, and tests that run through `photocraft-cli`.

## Releases

Each plug-in is versioned on its own: a tag such as `odc-film-look-v2.0.0`, `odc-lens-v1.0.0`, `odc-glow-v1.0.0`, `odc-tone-v1.0.0`, `odc-mono-v1.0.0` or `odc-retro-v1.0.0` builds the plug-in on GitHub and publishes a release with the `.wasm` attached.

## Credit and redistribution

These plug-ins are copyright 2026 Open Designer Cloud and released under the Apache License 2.0. You can use, modify and redistribute them, including in commercial products, as long as you follow the license. In particular, when you redistribute the source or a modified version you must:

- keep the `LICENSE` file and the `NOTICE` file,
- keep the copyright and attribution notices in the source files,
- state any changes you made to the files.

Copying the code, changing the name and removing these notices does not meet the license. If you spot a copy that does not give credit, please [tell us](https://opendesignercloud.com/contact).

## License

The repository is licensed under the Apache License 2.0 (see `LICENSE`), and each plug-in also carries its own copy and `NOTICE` in its folder (ODC Film Look, ODC Lens, ODC Glow, ODC Tone, ODC Mono and ODC Retro: Apache License 2.0). Example photos are from Open Designer Cloud Stock Photos, https://opendesignercloud.com/stock/photos/ (royalty free).
