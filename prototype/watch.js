import { execFileSync } from "child_process";
import { writeFileSync, watch } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const GRASS_BIN = resolve(__dirname, "../target/release/grass");

const input = process.argv[2] || "style.scss";
const output = process.argv[3] || input.replace(/\.scss$/, ".css");

function compile() {
  try {
    const start = performance.now();
    const css = execFileSync(GRASS_BIN, [input, "--style=expanded"], {
      encoding: "utf-8",
    });
    const ms = (performance.now() - start).toFixed(1);
    writeFileSync(output, css);
    console.log(`[${new Date().toLocaleTimeString()}] compiled (${ms}ms)`);
  } catch (e) {
    console.error(`[${new Date().toLocaleTimeString()}] error:`, e.stderr || e.message);
  }
}

compile();
console.log(`Watching ${input}...`);
watch(input, { persistent: true }, (event) => {
  if (event === "change") compile();
});
