# DejaVu Sans Mono

The Linux build embeds these four faces and registers them at startup, so code,
diffs and hashes use the same monospace font on every distribution. GPUI's Linux
text system matches family names exactly and does not consult fontconfig
aliases; without an installed DejaVu Sans Mono (Arch and Omarchy do not install
it by default) code fell back to a proportional interface font. macOS keeps Menlo
and does not embed these files.

- Version: DejaVu fonts 2.37, unmodified.
- Source: `https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.tar.bz2`
  (sha256 `fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7`).
- License: Bitstream Vera Fonts license with public-domain DejaVu changes and
  Arev glyphs, preserved in
  [`docs/licenses/assets/dejavu-fonts-LICENSE`](../../../docs/licenses/assets/dejavu-fonts-LICENSE)
  and copied into every package's notices. Redistribution with software is
  allowed; a modified copy must be renamed, and the fonts may not be sold by
  themselves.

| File | sha256 |
| --- | --- |
| `DejaVuSansMono.ttf` | `b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582` |
| `DejaVuSansMono-Bold.ttf` | `bce60f1b4421acd9ea51ba6623d7024ecbe6817a953e3654df62a5e6bdf8f769` |
| `DejaVuSansMono-Oblique.ttf` | `742097840c541870e8d6dc5c9b37bb1ceeea6c0dedd1d475faf903ef9df734b0` |
| `DejaVuSansMono-BoldOblique.ttf` | `91713a71d550bba22c2a6b2bb2a9ad8f9a159e12e4e9f0a5b2677998ba21213e` |
