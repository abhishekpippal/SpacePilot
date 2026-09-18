# SpacePilot 0.6.0 Timeline stall investigation

## Status

The algorithmic bottleneck has been corrected and measured on synthetic data.
The original approximately 534K-file Windows scan still requires one successful
manual retest before the reported real-world bug can be called fixed.

## Confirmed bottleneck and call path

`App.start -> scan_folder -> scanner::scan -> timeline::record_snapshot_cancellable
-> snapshot_cancellable -> developer_storage::analyze_cancellable -> retain -> save`.

The old snapshot builder called Developer Storage analysis. For every nonempty
directory, that analysis searched the entire file list for a nearby .NET marker.
It normalized/allocated parent paths on each comparison. With 533,929 files and
65,120 directories, the worst-case search involves 34,769,456,480 comparisons.
The old finding builder also searched all directories/files for each finding and
rescanned all files for each directory's descendant count. Candidate ownership
compared candidates pairwise.

The user's observed SAVING_TIMELINE boundary and development diagnostic were
confirmed. A controlled benchmark reproduces the expensive old marker loop.
The old full real-drive run was stopped, not allowed to complete; there is no
measured old full-snapshot duration or old real-drive serialization/write timing.
The earlier claim that React transfer was the confirmed cause was too strong:
that change did not resolve this Timeline bottleneck.

Timeline uses authoritative Rust state. It does not obtain results from React,
recalculate directory byte totals, hash file contents, run duplicate detection,
generate cleanup recommendations, or generate a Space Rescue plan. It does run
metadata-only Developer Storage classification. Scope identity hashes only the
scope identifier strings, not file contents. Filesystem statistics were obtained
before Timeline starts.

## Implementation and complexity

- Index .NET marker parents once; directory checks use set membership.
- Index normalized directory/file identities for finding lookup.
- Count descendants once by walking file ancestors; reuse scan directory bytes.
- Sort ownership candidates by path, then check ancestor membership. Outermost
  ownership, same-path priority, and exclusion of overlapping totals are retained.
- Select the top 500 directories and top 2,000 qualifying files using partition
  selection; sort only retained references and clone only persisted entries.
- Add cancellation checks to analysis loops and before snapshot commitment.
- Write and flush a unique temporary file, then replace history. On a failed or
  cancelled commit, remove only that operation's temporary file and preserve history.
- Refuse to overwrite unreadable, corrupt, or unsupported snapshot history.
- Return successful scan results with a Timeline warning on snapshot failure or
  cancellation, including Timeline state-lock errors.

With N files, D directories, C candidates and depth H, previous work included
O(N*D + C*(N+D) + C^2), plus full candidate sorts and repeated path allocation.
Current expected hash-index work is O(N*H + D + C*H + C log C), plus linear top-K
selection and sorting the retained K entries. Path processing depends on path
length. Index memory is linear in input paths and their distinct ancestors.

## Measured performance (Windows debug test build)

These are hardware observations, not performance guarantees.

| Measurement | Before | After |
| --- | ---: | ---: |
| Marker search, 10,000 files / 100 directories | 1,056.648 ms | 1.892 ms |
| Full original real-drive Timeline run | User observed >20 minutes, never completed | Manual retest pending |
| 550K synthetic snapshot construction, final pipeline run | Not run with old quadratic implementation | 8,992.716 ms |
| Full synthetic history serialization (2 snapshots) | Not measured | 171.775 ms |
| Temporary write and flush | Not measured | 3.778 ms |
| Replacement | Not measured | 1.683 ms |
| Full synthetic Timeline pipeline | Not measured | 9,238.031 ms |

Other measured final-pipeline stages: developer analysis 8,667.472 ms;
directory selection/sort 35.352 ms; large-file selection/sort 211.992 ms;
category/scope construction 0.187 ms; retention 0.219 ms.
Earlier standalone snapshot runs measured 8.63–9.90 seconds.

The synthetic fixture contains 550,000 in-memory file records, 70,000 directories
and 70,000 developer candidates. Files have sizes, categories and modification
timestamps; every synthetic file qualifies for the large-file threshold, exercising
selection across the full input. No 550K-file physical tree is created.

## Payload and existing history

- Existing real history: 19,205 bytes, 2 snapshots, 1 deletion-attribution record.
- Rust parse: 1.749 ms; read plus parse: 2.729 ms; no warnings.
- Synthetic snapshot: 348,605 bytes, exactly 500 directories and 2,000 files.
- Synthetic one-snapshot history: 348,675 bytes.
- Synthetic two-snapshot history: 697,281 bytes.
- The existing real history was inspected read-only and was not deleted/reset.
- Benchmark persistence uses a unique disposable temporary directory, removed
  after the test. No user scan files are written or deleted.

## Diagnostics

Development-only elapsed-millisecond logs cover history loading/parsing,
developer analysis, directory/file selection and sorting, category/scope building,
snapshot construction and payload measurement, retention, serialization, temporary
write/flush, replacement, and total recording time. Logs report counts/bytes without
private paths or contents. UI retains distinct scan phases and describes hidden
developer-category work within the Timeline activity.

## Validation and remaining limits

Final validation: `cargo fmt --check`, `cargo check`, and `cargo test` passed
(78 normal tests, zero failures). Both ignored performance tests were also run
explicitly and passed. Frontend tests passed (22), as did TypeScript checking and
the production frontend build. Backend tests were not run because backend code
was unchanged. The 4,000-record analysis-cancellation test returned at checkpoint
51 in 8.925 ms including initial index setup; this is not a full-drive UI latency
measurement. Cancelled temporary writes left existing history byte-for-byte intact.

Four normal regression tests were added: indexed marker/count correctness,
analysis cancellation, cancelled-write preservation, and corrupt-history
preservation. Two explicit performance tests cover the old/new marker search and
the 550K Timeline pipeline. Existing retention, scope compatibility, category,
ownership, cleanup-attribution, filesystem-safety and SHA-256 tests remain.

Timeline persistence remains serialized within the scan worker. Results return
after this now-measured phase completes, fails, or acknowledges cancellation;
they are not published to React while persistence is still running. This avoids
introducing concurrent history writers and overlapping scan-state changes.
Synchronous filesystem writes/flushes cannot be forcibly interrupted safely;
cancellation is checked before replacement and cannot undo an already committed
snapshot. Manual Windows button/cancellation behavior still needs confirmation.

No backend changes, version change, installer generation, deployment, or cloud
upload was performed. Benchmark gains do not prove the original drive retest.

## Manual retest

In the development app, select the same drive/location and let the scan finish.
Observe Scanning, Analyzing, Finalizing and Saving Timeline. Record the duration
of Saving Timeline and confirm the Dashboard becomes usable. If it pauses,
preserve the phase and timing output. If necessary, Cancel scan during Timeline
should return the completed scan with a warning and preserve previous history.
