# Bundled monospace font

`HyprkeyMono.ttf` is a subset of JetBrainsMonoNerdFontMono-Regular.ttf from
`ttf-jetbrains-mono-nerd`. It contains Latin text and the symbols used by hyprkey.
The JetBrains Mono letterforms and fixed advances are preserved. The subset
keeps the upstream font naming metadata; it is not a new font design.

Upstream: https://github.com/JetBrains/JetBrainsMono
Nerd Fonts distribution: https://github.com/ryanoasis/nerd-fonts
License: SIL Open Font License 1.1, included in `OFL.txt`.

Reproduce with fonttools:

```sh
pyftsubset /usr/share/fonts/TTF/JetBrainsMonoNerdFontMono-Regular.ttf \
  --output-file=assets/fonts/HyprkeyMono.ttf \
  --unicodes='U+0020-007E,U+00A0,U+00D7,U+2190-2198,U+21A5,U+21DE-21E5,U+21E7,U+21EA,U+2212,U+2303,U+2318,U+2325,U+2326,U+2328,U+232B,U+238B,U+23CE,U+2423,U+2699' \
  --name-IDs='*' --name-legacy --name-languages='*'
```
