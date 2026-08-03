// Runs pure Rust test binaries without requiring the Windows C++ linker.
// Hardware/Linux adapter tests still run natively on Raspberry Pi OS.
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";

const modulePath = process.argv[2];
if (!modulePath) {
  throw new Error("expected a WebAssembly test module path");
}

const wasi = new WASI({
  version: "preview1",
  args: [modulePath, ...process.argv.slice(3)],
  env: process.env,
  preopens: { ".": process.cwd() },
});
const module = await WebAssembly.compile(await readFile(modulePath));
const instance = await WebAssembly.instantiate(module, {
  wasi_snapshot_preview1: wasi.wasiImport,
});
wasi.start(instance);

