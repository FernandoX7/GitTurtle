# Native codec fixtures

`half-red-blue.heic` and `half-red-blue.avif` are generated 64 × 64 images: the top 32 rows are red and the bottom 32 rows are blue. No third-party artwork or metadata is included. They were encoded with macOS 26.6.2 ImageIO through `sips -s format heic` / `sips -s format avif` from a deterministic RGB PNG on September 9, 2026.

The decoder tests use misleading filenames deliberately. They verify container-byte routing, native codec decoding, dimensions, bounded resizing, and top/bottom orientation, allowing lossy color variation. Full rendering requires compatible macOS ImageIO codecs; Linux has a precise unsupported-codec fallback.

`half-red-blue.jp2` uses the same deterministic 64 × 64 RGB pattern, encoded with
macOS ImageIO `sips -s format jp2`. `half-red-blue.j2k` is its extracted `jp2c`
codestream. Both contain only generated test pixels and test supplied-byte
JPEG 2000 detection, rendering, orientation, and bounded resize independently
from their filename. No original artwork or private metadata is included.

Historical macOS 26.6.2 validation rendered both JPEG 2000 fixtures. A separate
[macOS 15.7.9 native probe](../../../../docs/benchmarks/jpeg2000-macos15-20260914.json)
([hosted run](https://github.com/FernandoX7/GitTurtle/actions/runs/34886145319))
rendered JP2 with correct dimensions and red/blue orientation through both direct
ImageIO and thumbnail APIs; both APIs returned no image for raw J2K. Recognition
as `public.jpeg-2000` alone does not establish raw-codestream rendering support.

The JP2 test always requires native pixels on macOS. The raw J2K test independently
probes direct ImageIO decoding of this fixed 64 × 64 fixture: a capable codec must
pass every production pixel, dimension, orientation and resize assertion; an
incapable codec must produce the exact unsupported result while byte detection
still overrides the misleading filename. Source-creation or unexpected-dimension
failures in that probe fail the test. There is no OS-version or environment skip.

`disposal-transparency.gif` is a generated 4 × 2 indexed-color animation encoded
with Pillow 12.1.1. Its three frames contain red, then green over transparent
pixels, then blue over transparent pixels; delays are 70/90/110 ms and disposal
modes are Keep/Background/Keep. It verifies that transparent overlays retain the
previous canvas and background disposal clears it before the next frame.
