# prototype/

Benchmark fixtures and perf tooling for grass. See `docs/design/performance-roadmap.md`
for how these are used.

- `perf-check.sh` — pre-commit perf gate against the USWDS fixture (`packages/uswds`, untracked).
- `bench.sh` — cross-engine benchmark (native/napi/WASM/sass-embedded) against USWDS.
- `fetch-bootstrap.sh` — fetches the Bootstrap v5.0.2 A/B workload (untracked, not used by
  `perf-check.sh`) into `bootstrap-bench/`.
