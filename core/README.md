# sen-core

`sen-core` is the emulation library used by the SEN frontends. It handles the
CPU, PPU, APU, cartridges, controllers, video frames, audio samples, save RAM,
and emulator state. Windowing, input devices, and audio playback are left to the
caller.

The main entry points are `Cartridge` for loading an iNES ROM and `Nes` for
running it. A frontend supplies controller state for each frame, then reads the
resulting frame and audio samples.

Run this crate's tests from the workspace root:

```sh
cargo test -p sen-core
```
