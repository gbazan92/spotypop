# Third-party notices

SpotyPop is licensed under the GNU General Public License v3.0 (see `LICENSE`).
It is an adaptation of the two projects below, released under the MIT License.
Their copyright and permission notices are reproduced here, as that license
requires.

## omarchy-spotify

- Project: https://github.com/ninepointlabs/omarchy-spotify
- Author: Ninepoint Labs
- Used for: the panel and popup design, and the Spotify Web API client (PKCE
  login, playback control, library and search), ported from its
  `bin/spotify-bridge` through the fork
  [gbazan92/omarchy-spotify-client](https://github.com/gbazan92/omarchy-spotify-client).

```
MIT License

Copyright (c) 2026 Ninepoint Labs

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

## Omarchy-Spotify

- Project: https://github.com/stappmus/Omarchy-Spotify
- Author: Kristoffer Haugland ([@stappmus](https://github.com/stappmus)) and the
  Omarchy Spotify contributors
- Used for: the local Spotify Connect receiver (`spotypop-player`), adapted from
  its `omarchy-spotify-backend`, including the choice of librespot fork.

```
MIT License

Copyright (c) 2026 Omarchy Spotify contributors

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

## Libraries

SpotyPop is built on [libcosmic](https://github.com/pop-os/libcosmic) (MPL-2.0)
and plays audio with [librespot](https://github.com/librespot-org/librespot)
(MIT, Copyright (c) 2015 Paul Lietar), through the fork
[stappmus/librespot](https://github.com/stappmus/librespot). Every other Rust
dependency keeps its own license, listed in its crate.

## Trademarks

Spotify is a trademark of Spotify AB. COSMIC is a trademark of System76, Inc.
SpotyPop is an independent project, not affiliated with or endorsed by either.
