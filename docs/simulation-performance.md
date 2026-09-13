# Headless simulation performance

Run the release, renderer-free simulation harness for the two stress workloads used by the game:

```sh
./scripts/simulation-performance
```

The wrapper builds `simulation_performance` once, then runs **8 NPCs** and **12 NPCs** serially
with the same seed. Defaults are 3,600 ticks, 300-tick windows, seed 42, a 900-second build
timeout, and a 120-second timeout per run. Build parallelism defaults to `CARGO_BUILD_JOBS=2`
and test-thread configuration to `RUST_TEST_THREADS=2`; either environment variable can be
provided by the caller. Use `--no-build` for an existing release binary.

The binary is resolved from `CARGO_TARGET_DIR` when set (relative values are resolved from the
repository root). Raw JSONL is written by default to:

```text
target/performance/simulation-performance-8-npcs.jsonl
target/performance/simulation-performance-12-npcs.jsonl
```

Choose another raw-output prefix explicitly, for example:

```sh
./scripts/simulation-performance \
  --ticks 18000 --window 600 --seed 42 \
  --output target/performance/soak-01
```

This also writes `soak-01-comparison.txt`. `--phase-timing` forwards the example's optional
phase instrumentation. The command stops on a build/run timeout and leaves any partial JSONL for
diagnosis.

Process CPU timing is an opt-in Linux-only diagnostic on the example. Run the binary directly with
`--cpu-timing` (it can be combined with `--phase-timing`):

```sh
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 timeout 300 \
  cargo run --release --no-default-features --example simulation_performance -- \
  --ticks 3600 --window 300 --npcs 8 --seed 42 --cpu-timing
```

Without `--cpu-timing`, the JSON is unchanged and no process-CPU clock reads are made. With it,
each window gains `cpu_timing` containing process-CPU `mean_ms`, nearest-rank `p95_ms`, `max_ms`,
`ticks_over_16_67_ms`, a paired `wall_max_sample` (`tick`, `wall_ms`, `cpu_ms`), and
`wall_over_33_33_cpu_under_16_67`. The last value is a descriptive count of samples whose wall
time exceeded 33.33 ms while process CPU was at most 16.67 ms; it does not prove that the
simulation thread was preempted. The paired sample uses the first sample when wall maxima tie.

The clock is `CLOCK_PROCESS_CPUTIME_ID`: it includes CPU consumed by all Bevy worker threads and
can exceed wall time during parallel work. It is a process-CPU diagnostic, not an FPS measurement.
The CPU-instrumented run adds clock-call overhead, and clock failures (including a backwards
sample) fail the run rather than being reported as zero. Enabling it on another OS is rejected
explicitly.

Latest 36,000-tick CPU-instrumented runs found maximum process-CPU samples of 28.26 ms
(8 NPCs, seed 42), 22.40 ms (12 NPCs, seed 42), 24.70 ms (12 NPCs, seed 7), and
33.89 ms (12 NPCs, seed 123). Paired wall/CPU measurements show that some remaining
outliers consume real CPU time, rather than establishing scheduler preemption as the cause.
Raw logs: `target/performance/cpu-{8,12}.jsonl` and `cpu-12-seed-{7,123}.jsonl`.
A tested cross-route spatial-query cache changed means by only about −3% to +2% across
three runs and added substantial state; it was removed. Its experimental `cached-*.jsonl`
logs do not describe retained code. Hardware-browser performance remains unverified.

The comparison reports, independently for each workload:

- a **weighted mean** across windows (weighted by each window's `samples`);
- the **maximum window p95**, explicitly **not a global p95**;
- the maximum individual tick (`max_ms`); and
- the total ticks over 16.67 ms.

These are native headless CPU timings including authoritative snapshot work, not rendered frames.
They must not be presented as browser FPS or as a physical GPU result. For browser pacing and
hardware validation, use the procedures in [web-performance.md](web-performance.md).

Focused wrapper tests do not invoke Cargo:

```sh
python3 scripts/simulation_performance_test.py
```
