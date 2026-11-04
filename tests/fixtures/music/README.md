These are original synthetic 12-second 440 Hz tones, attenuated by 0.02.
No recorded or third-party music is included. The fixtures cover different
sample rates, channel counts and sample formats:

- `tone.flac`: 48 kHz, stereo, 24-bit lossless, Vorbis comments (including Unicode).
- `tone.wav`: 44.1 kHz, mono, 16-bit PCM, RIFF INFO tags.
- `tone.ogg`: 48 kHz, stereo, Ogg Vorbis, Vorbis comments.

Generated with FFmpeg (a fixture-generation tool, not a DN runtime requirement):

```sh
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=48000:duration=12 \
  -af volume=0.02 -ac 2 -c:a flac -sample_fmt s32 -bits_per_raw_sample 24 \
  -metadata title='FLAC 界' -metadata artist='DN test artist' \
  -metadata album='Format tests' tone.flac
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100:duration=12 \
  -af volume=0.02 -c:a pcm_s16le -metadata title='WAV title' \
  -metadata artist='DN test artist' -metadata album='Format tests' tone.wav
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=48000:duration=12 \
  -af volume=0.02 -ac 2 -c:a vorbis -strict experimental -q:a 2 \
  -metadata title='Vorbis title' -metadata artist='DN test artist' \
  -metadata album='Format tests' tone.ogg
```

Normal tests decode bounded samples without an audio device. The ignored music
playback walkthrough explicitly sends these tones to the default macOS output.
