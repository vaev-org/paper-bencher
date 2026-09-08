# paper-bencher

A small, sequential benchmark runner for comparing two `paper-muncher`
versions. Run it from inside a paper-muncher Git checkout.

## Install

During development:

```bash
cargo install --path .
```

Once this repository has a remote:

```bash
cargo install --git <repository-url> paper-bencher
```

The runtime dependencies are `python3`, `hyperfine`, `perf`, `heaptrack`,
`heaptrack_print`, `heaptrack_gui`, `setsid`, and `git`. The default flow also
requires the checkout's `./ck` launcher and its build dependencies.

The general-ledger Python generator and its stylesheet are bundled into the
binary, so the installed command does not depend on the original `samples/`
directory. Ledger sizes must be multiples of 100, matching the existing
generator's sizing model.

## Use

```bash
cd /path/to/paper-muncher

paper-bencher run before --ledger-size 2000
# Make or checkout the change.
paper-bencher run after --ledger-size 2000

paper-bencher compare before after
```

Common run settings have short flags:

```bash
paper-bencher run before -s 2000 -w 2 -r 10
```

- `-s, --ledger-size`: generated ledger size (default `2000`; multiple of 100)
- `-w, --warmup`: Hyperfine warmup count (default `1`)
- `-r, --runs`: Hyperfine measured run count (default `5`)
- `--perf-frequency`: perf sampling frequency (default `997`)
- `--perf-call-graph`: `dwarf`, `fp`, or `lbr` (default `dwarf`)
- `--only`: run only a comma-separated selection of `hyperfine`, `perf`, and `heaptrack`
- `--skip`: run everything except a comma-separated selection
- `-b, --binary`: skip the default build and benchmark this executable
- `-f, --force`: replace an existing labeled run after the new run succeeds

Examples:

```bash
# Fast timing-only comparison
paper-bencher run before --only hyperfine -w 1 -r 5
paper-bencher run after  --only hyperfine -w 1 -r 5
paper-bencher compare before after

# CPU profile without heaptrack
paper-bencher run cpu-profile --skip heaptrack

# Run two selected profilers
paper-bencher run selected --only hyperfine,perf
```

`--only` and `--skip` cannot be combined. At least one profiler must remain
enabled. Two runs must use the same profiler selection before they can be
compared.

## Compare Git revisions directly

Build and compare two branches, tags, commits, or other Git revisions without
switching the current checkout:

```bash
paper-bencher compare-refs main feature/remove-flags -s 2000 -r 5
paper-bencher compare-refs v0.7.0 v0.7.1 --only hyperfine
```

Paper Bencher creates detached temporary worktrees, runs the benchmarks
sequentially, stores both runs in the current checkout's `.bench/` directory,
generates the comparison, and removes the temporary worktrees.

Compare a clean revision with the current working tree, including staged and
unstaged changes:

```bash
paper-bencher compare-working-tree HEAD -s 2000
paper-bencher compare-working-tree main --only hyperfine -r 10
```

This provides the usual “stashed versus unstashed” comparison without changing
or stashing the current checkout.

Git comparisons snapshot the current checkout's `.cutekit/externs` into each
temporary worktree by default. This keeps manually selected Karm branches or
commits identical across both builds when paper-muncher and Karm change
together. The copy uses filesystem reflinks when available, so it is normally
fast and space-efficient. To ignore local externs and let `ck` resolve each
ref's `project.json`, use:

```bash
paper-bencher compare-refs main feature --externs fresh
paper-bencher compare-working-tree HEAD --externs fresh
```

## Progress display

When attached to a terminal, long generator, build, benchmark, and report
commands show a rotating indicator on one line. It becomes `✓` on success or
`✗` on failure. Full command output is still written to the corresponding log
artifact.

By default, each run executes the equivalent of:

```bash
./ck package --release --prefix=<run-staging>/package paper-muncher
```

It benchmarks that package's `bin/paper-muncher`, then retains the exact binary
with the run artifacts. Select an already-built Paper Muncher or a completely
different executable with `--binary`; this skips `ck package`:

```bash
paper-bencher run debug --ledger-size 500 --binary ./path/to/paper-muncher
```

Results are kept under `.bench/<label>/`. A run label is never overwritten
unless `--force` is passed. Comparison reports are written to
`.bench/compare-<before>-<after>/`.

Every completed run that includes heaptrack automatically opens its profile in
`heaptrack_gui`. The GUI runs in the background, so `paper-bencher` continues
or exits without waiting for that window to close. During `compare-refs` and
`compare-working-tree`, the first run's GUI stays open while the second run is
collected; the second GUI opens when its run completes. The final comparison
does not open duplicate windows. A standalone `compare` opens both saved
profiles because it did not run them itself. Use `--skip heaptrack` or an
`--only` list without heaptrack when GUI windows are not wanted.

Profiling itself uses `heaptrack --record-only`. Heaptrack therefore never
opens or waits for a GUI between the first and second run; both windows are
opened only after all collection and report generation has completed.

Add `.bench/` to the target paper-muncher checkout's Git ignore rules if you do
not want benchmark artifacts to appear as untracked files. The tool itself
excludes `.bench/` when recording the run's `dirty` metadata flag.
