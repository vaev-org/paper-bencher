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
`heaptrack_print`, and `git`. The default flow also requires the checkout's
`./ck` launcher and its build dependencies.

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

Add `.bench/` to the target paper-muncher checkout's Git ignore rules if you do
not want benchmark artifacts to appear as untracked files. The tool itself
excludes `.bench/` when recording the run's `dirty` metadata flag.
