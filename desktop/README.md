# sen-desktop

`sen-desktop` is a small desktop frontend for SEN. It opens an iNES ROM,
renders frames to a window and sends audio to the default output device.

Run it from the workspace root:

```sh
cargo run --release -p sen-desktop -- path/to/game.nes
```

## Controls

| Key        | NES input |
| ---------- | --------- |
| Arrow keys | D-pad     |
| Z          | A         |
| X          | B         |
| Left Shift | Select    |
| Enter      | Start     |
| Escape     | Quit      |

For cartridges with battery-backed RAM, save data is read from and written to
a `.sav` file beside the ROM.
