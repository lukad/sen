# @lukad/sen

WASM bindings for SEN, a NES emulator.

This package is meant for browser apps. It ships ESM plus a `.wasm` file, so use it with a bundler.

## Installation

```sh
npm install @lukad/sen
```

```sh
pnpm add @lukad/sen
```

```sh
bun add @lukad/sen
```

## Usage

```ts
import initSen, { Emulator } from "@lukad/sen";

await initSen();

const rom = new Uint8Array(await file.arrayBuffer());
const audio = new AudioContext();
const emulator = new Emulator(rom, audio.sampleRate);

const imageData = context.createImageData(
  emulator.frameWidth(),
  emulator.frameHeight(),
);

emulator.runFrame();
imageData.data.set(emulator.frameBuffer());
context.putImageData(imageData, 0, 0);

const samples = emulator.takeAudio();
```

Controller masks use the NES button order:

```ts
const A = 1 << 0;
const B = 1 << 1;
const Select = 1 << 2;
const Start = 1 << 3;
const Up = 1 << 4;
const Down = 1 << 5;
const Left = 1 << 6;
const Right = 1 << 7;

emulator.setController1(A | Right);
```

Call `free()` when discarding an emulator instance.
