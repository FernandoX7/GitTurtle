# IBM Plex Sans

The Linux build embeds these six faces and registers them at startup, so the
interface draws its regular, medium, semibold and bold labels in real weights on
every distribution. The toolkit's Linux system font, `.SystemUIFont`, is IBM Plex
Sans. Without an installed copy (Arch and Omarchy do not install it) the toolkit
fell back to the desktop's first sans family at weight 400; Adwaita Sans and
Cantarell are variable fonts that the toolkit loads as one Regular face, so every
interface weight drew Regular. The italics serve Markdown emphasis. macOS keeps
its system font and does not embed these files.

- Version: IBM Plex Sans 3.005, from the `@ibm/plex-sans@1.1.0` release
  (`fonts/complete/ttf/`), unmodified.
- Source: `https://github.com/IBM/plex/releases/download/%40ibm/plex-sans%401.1.0/ibm-plex-sans.zip`
  (sha256 `fb365d910566e6d199cc2c15579a7dd9a267128e18431a394ed81f1970c69200`).
- License: SIL Open Font License 1.1, copyright IBM Corp. with Reserved Font Name
  "Plex", preserved in
  [`docs/licenses/assets/ibm-plex-sans-LICENSE.txt`](../../../docs/licenses/assets/ibm-plex-sans-LICENSE.txt)
  and copied into every package's notices. Bundling with software is allowed and
  the fonts may not be sold by themselves. A modified copy, including a subset,
  may not use the name "Plex", so keep these files whole and unrenamed.

| File | sha256 |
| --- | --- |
| `IBMPlexSans-Regular.ttf` | `975dcda37d80f038dcd143c22e33ca2d97a0cc5a929aace1c749153b0fe1afa5` |
| `IBMPlexSans-Medium.ttf` | `331c8639d7598b2cde62a911a71db195e30cb655cd6bdf2e324a7e984955f907` |
| `IBMPlexSans-SemiBold.ttf` | `a20caf8286023a6a7a85e40b1d2a4ae9fc3e3b1f9eda8f4c542dd4986af67bb1` |
| `IBMPlexSans-Bold.ttf` | `9e6c74a889a700d707613d24548fe4ffa6bc59559a0689d2cf9e133bdcdafb2f` |
| `IBMPlexSans-Italic.ttf` | `a9c6ef9942c49e49d11e11a6dacc0b3a087978757e9b22a06b8ac22a6400fb15` |
| `IBMPlexSans-BoldItalic.ttf` | `0e3142ba2ef31fe5c02f0c6c36424f251609cd6b73880076e21c2e81931ba2b9` |
