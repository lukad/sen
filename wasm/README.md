# sen-wasm

`sen-wasm` exposes SEN to JavaScript through `wasm-bindgen`. Its `Emulator` API
accepts iNES ROM data and provides frame, audio, controller, save RAM and state
methods suitable for a browser frontend.

Build the WebAssembly crate from the workspace root:

```sh
rustup target add wasm32-unknown-unknown
cargo build --release -p sen-wasm --target wasm32-unknown-unknown
```

The generated module still needs to be processed by `wasm-bindgen`. The
[`web` package](../web/README.md) contains the packaged JavaScript interface and
a usage example.
