# smart_cut

Almost-lossless video cutter in Rust. Uses FFmpeg (ffmpeg-next) to slice video files with minimal re-encoding.

## How it works

1. Find nearest keyframes around start/end timestamps
2. Re-encode only tail segments (start→first_kf, last_kf→end)
3. Copy middle segment (first_kf→last_kf) losslessly — no re-encode
4. Concatenate in output container

## Stack

- Rust (edition 2021)
- `ffmpeg-next` 8.0 — FFmpeg bindings
- `clap` 4.5 — CLI args
- `thiserror` — error types

## Structure

```
src/main.rs       — CLI, Saw2 struct, seek, copy, reencode logic
src/reencoder.rs  — Transcoder: video decode→encode pipeline
src/saw.rs        — Saw struct (older/alternate approach)
samples/          — test input videos
output/           — output directory
```

## Build & run

```
cargo run -- --input <in> --output <out> --start <s> --end <e>
```

Or use `just try` (reads input/start/end from justfile).

## State

Active development. Core logic works but still rough — timestamp handling, seeking, and flush behavior may need fixes. Currently hardcoded to re-encode start→first_kf only; the full three-segment pipeline is partially wired up.
