# SEN

SEN is a Nintendo Entertainment System emulator written in Rust. The emulator is
split into a reusable core and a few small frontends and bindings.

## Workspace

- [`core`](./core) contains the emulation library
- [`desktop`](./desktop) is a minimal desktop frontend
- [`libretro`](./libretro) provides a libretro core
- [`wasm`](./wasm) exposes the emulator through WebAssembly
- [`web`](./web) packages the WebAssembly bindings for browser apps

## Getting started

Run the test suite from the repository root:

```sh
cargo test --workspace
```

To launch the desktop frontend with an iNES ROM:

```sh
cargo run --release -p sen-desktop -- path/to/game.nes
```

ROM images are not included.

## License

SEN is available under the [MIT License](LICENSE).
