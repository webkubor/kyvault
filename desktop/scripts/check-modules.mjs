// Parse every UI module as a real ES module.
// `node --check file.js` treats the file as CommonJS and silently accepts
// module-only syntax, so it cannot catch a stray brace in a .js module.
import { readdir, readFile, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);
const ui = new URL("../ui/", import.meta.url).pathname;
const dir = await mkdtemp(join(tmpdir(), "ky-modules-"));
const files = (await readdir(ui)).filter((f) => f.endsWith(".js"));
let failed = 0;

for (const file of files) {
  const target = join(dir, file.replace(/\.js$/, ".mjs"));
  await writeFile(target, await readFile(join(ui, file), "utf8"));
  try {
    await run(process.execPath, ["--check", target]);
    console.log(`ok   ${file}`);
  } catch (err) {
    failed += 1;
    console.error(`FAIL ${file}\n${err.stderr}`);
  }
}
if (failed) {
  console.error(`${failed} module(s) failed to parse`);
  process.exit(1);
}
console.log(`${files.length} module(s) parsed`);
