# Omarchy theme fixtures

Each `<name>.toml` is an unmodified copy of `themes/<name>/colors.toml` from
[Omarchy](https://github.com/basecamp/omarchy), as installed by the Omarchy 4.0.4
package (copied on September 28, 2026). All 22 bundled themes are here: 17 dark
and 5 light (`catppuccin-latte`, `flexoki-light`, `lupine`, `rose-pine`, `white`).

`appearance::omarchy::palette::tests` parses every file, checks its mode and
requires the palette it maps to to have no readability findings. These files are
test inputs only and are not packaged with the application.

Omarchy is distributed under the MIT license:

```text
MIT License

Copyright (c) David Heinemeier Hansson

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
