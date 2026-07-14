import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const FIXTURES = resolve(dirname(fileURLToPath(import.meta.url)));
const REPO_ROOT = resolve(FIXTURES, "../..");

function candidates(kind) {
  const result = [];
  if (process.env.PERF_FIXTURE_DIR) result.push(process.env.PERF_FIXTURE_DIR);
  else if (kind === "uswds" && process.env.USWDS_FIXTURE_DIR) result.push(process.env.USWDS_FIXTURE_DIR);
  result.push(join(FIXTURES, "fetched", kind));
  result.push(FIXTURES);
  return result;
}

function usable(kind, root) {
  if (kind === "uswds") return existsSync(join(root, "packages/uswds"));
  if (kind === "bootstrap") {
    return existsSync(join(root, "scss/bootstrap.scss")) ||
      existsSync(join(root, "bootstrap-bench/scss/bootstrap.scss"));
  }
  throw new Error(`unknown fixture kind: ${kind}`);
}

export function resolveFixture(kind) {
  for (const root of candidates(kind)) {
    if (usable(kind, root)) {
      const normalizedRoot = kind === "bootstrap" && existsSync(join(root, "bootstrap-bench/scss/bootstrap.scss"))
        ? join(root, "bootstrap-bench")
        : root;
      return {
        kind,
        root: normalizedRoot,
        loadPath: kind === "uswds" ? join(normalizedRoot, "packages") : join(normalizedRoot, "scss"),
        entry: kind === "uswds"
          ? join(normalizedRoot, "packages/uswds/_index-direct.scss")
          : join(normalizedRoot, "scss/bootstrap.scss"),
      };
    }
  }
  const hint = process.env.PERF_FIXTURE_DIR
    ? `PERF_FIXTURE_DIR=${process.env.PERF_FIXTURE_DIR} has no ${kind} fixture`
    : `no ${kind} fixture is fetched or present in the legacy tree`;
  throw new Error(`${hint}; run bash bench/fixtures/fetch.sh ${kind}`);
}

export { REPO_ROOT };
