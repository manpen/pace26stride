# STRIDE --- The PACE26 Maximum-Agreement Forest companion
The STRIDE system is designed as a companion to the [PACE 2026 challenge](https://pacechallenge.org/2026/) on [Maximum-Agreement Forest](https://pacechallenge.org/2026/maf/).
It includes tools to execute solvers and verify their solutions.

> [!NOTE]  
> This is an early preview version that should already work as documented (please report issues if not).
> We are planning access to a large database of test instances and best known solutions.
> So ~~like and subscribe~~ watch out for updates.

## Feature overview
The `stride` tool is build as a single statically linked executable (i.e., you can freely move the binary on your machine) and offers subcommands for several tasks:
 - `stride check`: Check and visualize instances and solutions
 - `stride run`: Execute a solver (in parallel), verify and summarize solutions

You may use `stride --help` or `stride {subcommand} --help` for further information.

## Checker & Visualizer
The primary use case of the checker is to verify a solution computed by your solver by running
```bash
stride check <INSTANCE-PATH> <SOLUTION-PATH>
```

### Visualizing
By passing the parameter `-d/--export-dot` the checker will emit a visualization of a feasible solution in the [Graphviz DOT language](https://graphviz.org/doc/info/lang.html).
This feature is intended for small instances only.

You may directly pass the output into Graphviz's `dot` tool (there are also a number of online tools):
```bash
stride check --export-dot instance.nw solution.sol | dot -T pdf > solution.pdf
```

For the `tiny01.nw` instance this may yield:
![Screenshot: Render of tiny01](docs/dot_render.png)

Each tree of the input is visualized independently where inner nodes are labelled according to the [PACE26 format specification](https://pacechallenge.org/2026/format/#indices-of-inner-nodes).
Each tree of the solution corresponds to a fixed color (we currently only support ~8 colors).
A triangular node indicate the root of a tree in the agreement forest; they are always connected to their parent (if any) by a dashed line.
Removing dashed lines and contracting inner nodes with an out-degree of 1 yields the MAF.

### More checking
If the solution path is omitted, a number of linters and checks are carried out on the instance.
This feature is only useful if you create your own instances.

The optional `-p/--paranoid` enables additional linters/stricter rules (e.g., pertaining to whitespace).
The PACE rules *do not* require that solver solutions pass this stricter mode.

## Run summary
We produce a machine-readable summary of each run in `stride-logs/{RUN}/summary.json`.
It's a newline delimited JSON file, where each line represents the result of a task (i.e. solver run) formatted in JSON.
That is, each line has to be parsed individually, the file itself is not a valid JSON expression.
Common data processing libraries natively support this format, e.g., [Polars](https://docs.pola.rs/api/python/stable/reference/api/polars.read_ndjson.html) and [Pandas](https://pandas.pydata.org/pandas-docs/stable/reference/api/pandas.read_json.html) (by setting `lines=True`).

By default, we record the following columns:
 - `s_name`: Name of instance (default: filename of instance)
 - `s_instance`: Path to instance file
 - `s_stride_hash`: Hash value if instance is registered in the global stride database
 - `s_solution`: Path to solution file (stdout)
 - `s_result` Possible values: 
    - `Valid`: the return solution is a feasible agreement forest (size is ignored)
    - `NoSolution`: the solution did not contain a single tree
    - `Infeasible`: the solution contained at least one tree
    - `InvalidInstance`: instance could not be parsed by stride
    - `SyntaxError`: at least one line could not be parsed; did you write a log message to stdout instead of stderr?
    - `SystemError`: e.g., solver or instance not found
    - `SolverError`: e.g., solver terminated with non-zero exit code
    - `Timeout`: a `SIGKILL` was sent
 - `s_score`: If `s_result` indicates a valid solution, report the number of tree in the MAF.
 - [Profiling](#profiling) related columns

### Profiling
By default (can be disable using `--no-profile`), we collect performance metrics of the solver using POSIX's `getrusage` function and own measurements.
 - `s_wtime`: Walltime of solver run (end time - start time) in seconds
 - `s_utime`: User time as reported by `getrusage` in seconds
 - `s_stime`: System time as reported by `getrusage` in seconds
 - `s_maxrss`: Maximum resident set size reported by `getrusage` **in bytes** (for portability). Small values of a few megabyte may not be reliable.
 - `s_minflt`: Number of page reclaims (soft page faults)
 - `s_maxflt`: Number of page faults (hard page faults)
 - `s_nvcsw`: Number of voluntary context switches
 - `s_nivcsw`: Number of involuntary context switches

### Report custom data
A solver may add additional data by emmiting stride lines in the following format:

```text
#s {KEY} {VALUE}
```

where `{KEY}` (without quotation chars! we test with `(a-zA-Z0-9_)+` but more is likely to work) is used as the key in summary log  and `{VALUE}` is a valid JSON expression.
If a key is present multiple time in a solution, only the last value will be reported.
For this reason avoid the prefix `s_` which is internally used by `stide`.

## Known limitations
Please check and contribute [issues](https://github.com/manpen/pace26stride/issues) and [pull requests](https://github.com/manpen/pace26stride/pulls).

### No Windows support
PACE uses the [optil.io](https://optil.io/optilion/help) conventions on signalling timeouts.
This intrinsically relies on POSIX signals, which seem to be unsupported in Windows. 
Hence, we only support Linux and OSX. 
