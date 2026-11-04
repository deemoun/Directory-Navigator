# Third-party notices

## Turbo Vision for Rust

Source: https://github.com/aovestdipaperino/turbo-vision-4-rust

Published crate: https://crates.io/crates/turbo-vision/4.1.0

Version: `4.1.0`, registry source revision `a1e2d829e318121116fd22a3254862a211636f13`.

MIT License

Copyright (c) 2025 Turbo Vision Contributors

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

## DOS Navigator Tetris / Pentix

The figure tables, rotation rules, scoring and reference dialog arrangement in
`src/tetris` and `src/turbo/tetris` are adapted from DOS Navigator 1.51
`TETRIS.PAS` and its dialog resources by RIT Research Labs.

Based on TinyWeb Server by RIT Research Labs.

```text
Dos Navigator  Version 1.51  Copyright (C) 1991-99 RIT Research Labs

This programs is free for commercial and non-commercial use as long as
the following conditions are aheared to.

Copyright remains RIT Research Labs, and as such any Copyright notices
in the code are not to be removed. If this package is used in a
product, RIT Research Labs should be given attribution as the RIT Research
Labs of the parts of the library used. This can be in the form of a textual
message at program startup or in documentation (online or textual)
provided with the package.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the copyright
notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.
3. All advertising materials mentioning features or use of this software
must display the following acknowledgement:
"Based on TinyWeb Server by RIT Research Labs."

THIS SOFTWARE IS PROVIDED BY RIT RESEARCH LABS "AS IS" AND ANY EXPRESS
OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE
GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

The licence and distribution terms for any publically available
version or derivative of this code cannot be changed. i.e. this code
cannot simply be copied and put under another distribution licence
(including the GNU Public Licence).
```

## DOS Navigator CD-player-derived interface

The Music Player interface in `src/plugins/mp3_player.rs` adapts the
three-row counter glyph and compact/expanded arrangement from DOS Navigator
1.51 `CDPLAYER.PAS`. The play-order dialog in
`src/plugins/mp3_player/playlist.rs` follow `CDUTIL.PAS` (`TOrder`, track/time/
description rows and Add/Delete controls). Modern Rust audio playback and duration
probing do not use the original DOS CD drivers.

Based on Dos Navigator by RIT Research Labs.

```text
Dos Navigator  Version 1.51  Copyright (C) 1991-99 RIT Research Labs

This programs is free for commercial and non-commercial use as long as
the following conditions are aheared to.

Copyright remains RIT Research Labs, and as such any Copyright notices
in the code are not to be removed. If this package is used in a
product, RIT Research Labs should be given attribution as the RIT Research
Labs of the parts of the library used. This can be in the form of a textual
message at program startup or in documentation (online or textual)
provided with the package.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the copyright
notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.
3. All advertising materials mentioning features or use of this software
must display the following acknowledgement:
"Based on Dos Navigator by RIT Research Labs."

THIS SOFTWARE IS PROVIDED BY RIT RESEARCH LABS "AS IS" AND ANY EXPRESS
OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE
GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

The licence and distribution terms for any publically available
version or derivative of this code cannot be changed. i.e. this code
cannot simply be copied and put under another distribution licence
(including the GNU Public Licence).

```

## DOS Navigator workflow references

Based on Dos Navigator by RIT Research Labs. Advanced Find criteria, parameterized
user actions and ordered highlight groups follow behavior documented in the
original `FILEFIND.PAS`, `USERMENU.PAS`, `SETUPS.PAS` and English `DNHELP.HTX`.
Related selection, editor block commands/bracket navigation and viewer addressing
also reference `EDITOR.PAS`, `FVIEWER.PAS` and the original keyboard help; UTF-8
transformations and undo transactions use the current Rust editor.
SmartPad follows the quick-notes command documented in the original English
`DNHELP.HTX` and `DN.DNR`; its implementation reuses the current Rust editor.
The implementation uses DN's current Rust workers, controls and file safeguards.
The original DOS Navigator notice and license reproduced above are retained.

## DOS Navigator screen savers

Based on Dos Navigator by RIT Research Labs, Copyright (C) 1991–99.
The Star flight, Flash-light, Clock and Blackness effects and the two-list selection
window adapt DOS Navigator 1.51 `original-dn/IDLERS.PAS`, `SETUPS.PAS` and their
resource strings. The native Rust implementation uses prepared terminal scenes
instead of direct DOS video-memory access, and does not execute external `.SS`
programs. The original DOS Navigator notice and license reproduced above are retained.

## MP3 audio libraries (macOS)

Rodio 0.22.2: https://github.com/RustAudio/rodio/tree/v0.22.2

Rodio is used under its MIT license:

```text
Copyright (c) The Rodio Project Contributors

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

Rodio uses CPAL for audio output (Apache-2.0):
https://github.com/RustAudio/cpal

MP3 decoding and read-only ID3 metadata use unmodified Symphonia 0.5.5 (MPL-2.0). Its source and license
are available at https://github.com/pdeljanov/Symphonia/tree/v0.5.5 and
https://www.mozilla.org/en-US/MPL/2.0/ respectively.

## DOS Navigator archive panes

Archive-as-directory navigation and selected F5 extraction follow the behavior in
DOS Navigator 1.51 `original-dn/ARCVIEW.PAS`, `ARCHIVER.PAS` and its English help,
Copyright (C) 1991–99 RIT Research Labs. Based on TinyWeb Server by RIT Research Labs.
The RIT Research Labs license and disclaimer reproduced above apply to adapted
behavior. Native ZIP/TAR codecs and filesystem safeguards are implemented in Rust;
DOS external-archiver command execution is not carried over.

ZIP creation and its password-confirmation workflow are adapted from the behavior
of DOS Navigator 1.51 `ARCHIVER.PAS` (`MakeArchive`), RIT Research Labs. The modern
implementation uses built-in ZIP/AES codecs and the shared Transfer Center rather
than external DOS packers. AES support and ephemeral credential clearing use the
`zip` crate encryption feature and `zeroize`, under their respective crate licenses.
