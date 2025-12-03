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
