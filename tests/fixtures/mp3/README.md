`tone.mp3` is an original synthetic 12-second 440 Hz test tone, attenuated by
0.02 before MP3 encoding. No recorded or third-party music is included.

Generated with FFmpeg:

```sh
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=22050:duration=12 \
  -af volume=0.02 -c:a libmp3lame -b:a 24k tone.mp3
```

Normal tests decode this fixture without an output device. The ignored macOS
hardware walkthrough explicitly plays it through the default output device:

```sh
cargo test --release --locked --test terminal_walkthrough mp3_audio -- --ignored --nocapture
```
