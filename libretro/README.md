# sen-libretro

`sen-libretro` packages SEN as a libretro core. It accepts `.nes` content and
provides video, audio, two controller ports, save RAM, save states, rewinding, netplay and a small
set of frontend options.

Build the core from the workspace root:

```sh
cargo build --release -p sen-libretro
```

Install the resulting dynamic library together with
[`libsen_libretro.info`](libsen_libretro.info) as required by your libretro
frontend.
