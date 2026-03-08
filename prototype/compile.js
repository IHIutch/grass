import { writeFileSync, existsSync } from "fs";
import { compile } from "../crates/lib/pkg-publish/index.js";

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

try {
  const start = performance.now();
  const result = compile(input, {
    style: "expanded",
    loadPaths,
  });
  const ms = (performance.now() - start).toFixed(1);

  writeFileSync(output, result.css);
  console.log(`${input} -> ${output} (${ms}ms)`);
} catch (e) {
  console.error(e.message);
  process.exit(1);
}
