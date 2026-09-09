# Native codec fixtures

`half-red-blue.heic` and `half-red-blue.avif` are generated 64 × 64 images: the top 32 rows are red and the bottom 32 rows are blue. No third-party artwork or metadata is included. They were encoded with macOS 26.6.2 ImageIO through `sips -s format heic` / `sips -s format avif` from a deterministic RGB PNG on September 9, 2026.

The decoder tests use misleading filenames deliberately. They verify container-byte routing, native codec decoding, dimensions, bounded resizing, and top/bottom orientation, allowing lossy color variation. Full rendering requires compatible macOS ImageIO codecs; Linux has a precise unsupported-codec fallback.
