import { execFileSync } from "child_process";
import { writeFileSync, existsSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const GRASS_BIN = resolve(__dirname, "../target/release/grass");

// Parse args: node compile.js [input] [output] [-I loadPath]...
const args = process.argv.slice(2);
const loadPaths = [];
const positional = [];

for (let i = 0; i < args.length; i++) {
  if (args[i] === "-I" || args[i] === "--load-path") {
    loadPaths.push(args[++i]);
  } else {
    positional.push(args[i]);
  }
}

const input = positional[0] || "style.scss";
const output = positional[1] || input.replace(/\.scss$/, ".css");

if (!existsSync(input)) {
  console.error(`File not found: ${input}`);
  process.exit(1);
}

if (!existsSync(GRASS_BIN)) {
  console.error(`grass binary not found. Run: cargo build --release`);
  process.exit(1);
}

const grassArgs = [input, "--style=expanded"];
for (const lp of loadPaths) {
  grassArgs.push("-I", lp);
}

try {
  const start = performance.now();
  const css = execFileSync(GRASS_BIN, grassArgs, { encoding: "utf-8" });
  const ms = (performance.now() - start).toFixed(1);

  writeFileSync(output, css);
  console.log(`${input} -> ${output} (${ms}ms)`);
} catch (e) {
  console.error(e.stderr || e.message);
  process.exit(1);
}
