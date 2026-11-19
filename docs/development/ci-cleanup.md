# CI clean-up plan

The owner asked on 2026-11-17 for CI to be made as fast as possible without letting it
lie, from an example plan written for a JavaScript repository. This is that plan for
GitTurtle, drawn on 2026-11-18 from measurements. It is updated as each change lands,
with before and after figures from real runs.

Two rules outrank speed, in the owner's words:

- The full suite runs before anything reaches `main`. Selection only narrows
  pull-request feedback.
- Never get faster by hiding failures. No retries, wider timeouts or skipped tests. A
  check that needs a retry has a bug; find it.

Each change is one small pull request with before and after numbers from real runs, and
each workflow change gets a security review. Anything that needs an account or a secret
is on [the owner's list](#the-owners-list).

The owner decided on 2026-11-18 that nothing is bought: this is an open-source project,
CI runs on what GitHub gives a public repository for free, and the work is to make the
project use that well. No paid runner, runner vendor or storage is part of this plan.

## Where the time is

From an audit of the last 150 Quality runs (2026-10-06 to 2026-11-17, kept locally with
its scripts) and the 14 successful full runs of the current workflow shape:

| Figure | Median | p95 |
| --- | --- | --- |
| Full pull-request run, wall-clock | 42 min | 54 min |
| Runner time per full run | 146 min | 315 min |
| of which macOS | 63 min | 85 min |
| `Rust tests · macos-15`, running | 2,364 s | 3,085 s |
| `Rust tests · ubuntu-24.04`, running | 1,456 s | 2,261 s |
| `Rust tests · archlinux`, running | 1,281 s | 2,390 s |
| Waiting for a runner, by runner label | 2 to 4 s | 3 to 8 s (macOS 421 s at p99) |
| Setup of a Rust job | 54 to 76 s | 65 to 230 s |

Nothing is billed: standard runners are free for a public repository. The macOS test job
is the last to finish in 14 of 14 full runs, so the example's expectation that "it is
rarely the tests" does not hold here: the tests are 39 of the 42 minutes.

What the tests are made of, from 30 sampled job logs and a local measurement:

- One test binary, the app's, holds 4,200 to 5,300 of each job's summed test seconds, in
  about 2,950 tests (2,700 on macOS).
- A quarter of its tests finish in under 50 ms, so a process per test is not the cost.
  The typical test takes 0.6 s on Linux and 1.2 s on macOS. About 190 tests of five
  seconds or more, walks over sizes and scales, hold about half the time; the slowest
  run 57 to 66 s against nextest's 120 s limit.
- The tests already use every processor: summed test seconds over nextest's wall is 4.0
  on the Linux runners and 3.0 on macOS, their processor counts.
- Measured locally, a typical test is its own thread's work in the toolkit's layout plus
  set-up that starts 46 to 103 `git` processes and 92 to 196 threads.
- The suite's time nearly tripled in two weeks: at the start of November the macOS job
  ran about 1,910 tests in 715 s; by 2026-11-16 it ran 3,232 in 1,976 s. Without a budget
  it keeps growing.

## What this account can use

Read from primary sources on 2026-11-18
([platform facts](2026-11-18-ci-github-platform-facts.md),
[runners and test tools](2026-11-18-ci-runners-and-test-tools.md)) and on 2026-11-19
([the Ubuntu `apt-get` step](2026-11-19-ci-ubuntu-apt-step.md)):

- A merge queue needs a repository owned by an organization. Here the strict required
  check on the pull request is the gate before `main`, so the pull request's own run
  stays the full suite, macOS included.
- Standard runners have 4 processors on Linux and 3 on macOS. The account runs at most
  5 macOS jobs at once across all its repositories; a full run starts 4.
- GitHub's larger runners and every runner vendor cost money, and the vendor the
  example names takes organizations only. None is used: the plan stays on the free
  standard runners.
- The cache is full: 10.76 GB against its free 10 GB on 2026-11-19, once `main` had
  saved the six entries of the current keys (5.84 GB). The rest are entries of older keys
  that nothing reads; GitHub removes them after seven days without a read or, least
  recently read first, some time after the limit is passed (the list still held
  10.76 GB five hours after the sixth save). A
  change to the setup action's files, or a runner image with a newer C library, adds
  entries beside the old ones, so another cache family still does not fit.
- The label `ubuntu-24.04` is served from two clouds at once. In 64 retained job logs the
  runner's header says Azure for image versions 20261111.370.1 and 20261116.376.1 and AWS
  for 20261111.127.1 and 20261116.133.1; only the Azure builds are published with a
  software list. Each build has its own package inventory, and during a weekly rollout
  four are in use.
- GitHub announced on 2026-10-28 that standard Linux jobs may run on AWS as well as on
  Azure, and promises nowhere that the two match. They differ in two measured ways. A
  Linux test shard's own test time was 524 to 568 s in all 26 logs from AWS runners and
  476 to 925 s in the 18 from Azure runners. And each cloud's runners install packages
  from their own Ubuntu mirror, which apt leaves only when a fetch fails, never because
  it is slow.
- A cancelled run leaves a failed gate on its commit, so cancellation stays off `main`.
- nextest's `--partition slice:m/n` deals tests round-robin; nextest cannot split by
  recorded duration, and simulated from the logs it does not need to: the round-robin
  split is within 5 % of a duration-balanced one.

## Targets

The example's two minutes at the median cannot be met while the pull request itself must
run about 5,000 processor-seconds of tests per platform on free 3 and 4 processor
runners. The targets, in order:

1. On free runners with this account: a full pull-request run under 22 minutes at the
   median and 30 at p95 (from 42 and 54).
2. After the tests' own set-up cost is cut: under 15 minutes at the median.
3. Only if the repository ever moves to a free organization, whose merge queue costs
   nothing: feedback on a pull request under 10 minutes, with the full suite in the queue
   before `main`.

A documentation-only pull request takes 69 s today when it changes no image; it stays
under two minutes.

## The changes, biggest bucket first

| # | Change | Basis | State |
| --- | --- | --- | --- |
| 1 | Two shards for each test job (`--partition slice:<n>/2`), each building from the restored cache; shard 1 alone saves; the release preflight asks for every shard | macOS test job about 41 min to about 21, Ubuntu to about 14, Arch to about 13, simulated from the logs; two because a third macOS shard would pass the limit of 5 | **Done**, merged as #217 on 2026-11-19. Its run against the last unsharded run of the same base: wall-clock 28 min 44 s against 54 min 1 s; macOS shards 1,404 s and 1,652 s against 3,163 s; Ubuntu 1,671 s and 1,674 s against 2,921 s (the cache missed on both sides); Arch 763 s and 753 s against 2,454 s (a miss against two hits); runner time 185 min against 205. Per platform the shards ran exactly the unsharded run's tests (macOS 3,385 = 1,693 + 1,692; Ubuntu and Arch 3,766 = 1,883 + 1,883), and all five macOS jobs started without queueing |
| 2 | `Development tooling · macos-15`: find why its unit-test step takes 345 s against 176 s on Ubuntu, and bring the job under 5 minutes | Cut off at its 10-minute limit in 6 of 150 first attempts; it also holds a macOS lane for 9 minutes | **Analysed** on 2026-11-19 from 124 job logs and a local count; nothing changed yet. The four unittest steps are 454 s of the macOS job's 471 s median, against 205 of 221 s on Ubuntu; the agent-loop suite alone is 346 s against 172 s. Its 350 tests make 2,741 supervised launches of three processes each, and one interpreter start with imports takes 104 ms on the macOS runners against 50 ms on Ubuntu. The six cut-off attempts were the same job on a slower runner. The cause is inferred from ratios, so next is one measurement-only run on both platforms; then the suite split across the runner's processors, a cheaper launch, or both |
| 3 | Remove the reasons for today's retries: the nextest download (failed 4 times in 150 runs; fetched on every run), the artifact download in `Rust release` (retried in 3 of 60 jobs), package installation from a mirror that was down; move the nextest pin from 0.9.146 to 0.9.152 with it | By the second rule each is a bug to find | Not started |
| 4 | Cut what a GUI test does before it tests: the `git` processes and threads of opening a repository, and the cost of the long walks | The typical test takes twice as long on macOS as on Linux; process and thread creation is the candidate, to be measured on a hosted macOS runner first. Five or six tests a job ran over 60 s in `main`'s run of 2026-11-18 against nextest's 120 s limit, so on a slow runner the long walks are a failure waiting to happen | Not started; product code, so implementer rounds with reviews |
| 5 | A third macOS shard once change 2 frees a lane, or one build feeding several shards if a measurement shows it wins and it fits the free cache and artifact limits | 654 s of tests in the slowest of three shards against 959 s of two | After 1 and 2 |
| 6 | Cache hits, the Ubuntu key first: key the Ubuntu lanes on the packages a build reads (the dependency closure of the action's build packages, the selected build tools, pkg-config's view, the two pkg-config names nothing supplies and a tripwire on `/usr/local`) instead of the runner image's whole package inventory; then measure the hit rate over more than the 10 sampled runs (68 %, 54 of 80 restores); then, with the helper's next change, recompute the key at the end of a job and refuse the save when it changed, and key the build crates' `HOST_*` and `TARGET_*` variables | The label `ubuntu-24.04` is served from two clouds with different inventories: of 47 Ubuntu jobs that computed a key (2026-11-16 to 2026-11-19), 19 found no cache; in the first three sharded runs five of six Ubuntu test shards built everything (1,499 to 1,712 s against 1,150 s for the one that hit); three Ubuntu debug entries of 1.1 GB each sat in a 10 GB cache. A traced build read files of 75 packages, 71 of them in the closure, which holds 195 | **The key is done**, merged as #219 on 2026-11-19 after a security review decided the rule. In its two runs the x64 jobs on the AWS image (1,252 packages) and on the Azure image (1,248) computed one key, 197 packages of it, each in the closure mode; with the inventory those two images had computed two. Every lane was cold once. **Measured** on three manually dispatched runs of 2026-11-19: every Ubuntu job whose key had an entry hit it, on either cloud (9 of 9 x64 jobs, and the arm64 job in the one run before its new image), with the tests and the package checks green; an Ubuntu shard took 870 to 1,191 s on a hit against 1,410 to 2,021 s on a miss; the cache holds one Ubuntu debug entry of the current key where it held three. When the image release with a newer C library began to serve jobs, both clouds computed one new key for it, so a rollout gives two keys where the inventory gave four, and a lane on a new image is cold until `main`'s saving job has drawn one. The run did not get shorter (see the totals): with the Ubuntu shards done at 15 to 21 minutes it waits for a macOS shard. **The helper's next change is done**, merged as #221 on 2026-11-19 after a security consult decided its rules: the identity is computed again at the end of a job and a changed one refuses the save without failing the job; the build crates' variables are keyed by name; the directories a `PKG_CONFIG` variable names are listed; apt's `cmake` leaves the key (change 9). In its own run all six saving lanes printed that their identity was unchanged since setup, only `CARGO_INCREMENTAL` was keyed as a flag, and every lane was cold once more. `main`'s run on the merge saved the six entries with no save refused, and the run after it restored every key that had an entry, across the clouds in both directions (9 of 10 Rust jobs; the tenth drew the newer image release, whose x64 debug key nothing had saved) |
| 7 | Cancel a superseded manual run on the same branch | 106 of the 150 runs were manual and never cancelled; never on `main` | Not started |
| 8 | A budget for every job: its time limit set from the measured p95 with a stated margin, so growth fails a run instead of passing unseen | The suite's time nearly tripled in two weeks | After 1 to 5 |
| 9 | Setup seconds, package installation on Ubuntu first; then the toolchain inside the formatting step, the nextest download, checkout | Over 80 Ubuntu job logs of 20 runs (2026-11-16 to 2026-11-19) the action's `apt-get` step took 12 to 46 s on either cloud until 2026-11-18. Since that evening it has taken 61 to 718 s on every AWS runner (19 logs) and 52 to 290 s on six of the seven Azure runners since 06:26 UTC on 2026-11-19, because their Ubuntu mirrors serve at 30 to 608 kB/s where they served at 3.4 to 26 MB/s; nobody has published a cause ([research note](2026-11-19-ci-ubuntu-apt-step.md)). The step downloads 19.2 MB, and 13.4 MB of that is apt's CMake, which the build does not run on a hosted image (the image's own CMake is first on `PATH`). Elsewhere setup is 54 to 76 s a Rust job | **First part done**, merged with the helper's change as #221 on 2026-11-19: apt's `cmake` leaves the install list, and the image's own stays a required keyed tool. The install fetches 5.7 MB on x64 and 5.5 MB on arm64 where it fetched 19.2 and 18.1 MB. The AWS runners' mirror was no faster in the three runs since (20 to 130 kB/s), and the step took 79 to 327 s there (six jobs) against 242 to 718 s in the three runs before, 13 to 59 s on the Azure x64 runners and 24 to 26 s on arm64. Decided against, each for its reason: restoring cached `.deb` files (apt accepts a cached archive on its size alone, so its signature chain would not cover them), another mirror order (unmeasured, and it changes whose availability the step depends on), a narrower `apt-get update` (at most 7.2 of 13.2 MB, on the step's shorter half). The slow mirrors are a third party's; the same count over later runs says when they recover |
| 10 | Start the eight Image privacy jobs only when the routing job finds an added or changed image | About 20 s each in every run, eight queue slots | Not started |
| 11 | Local, under 9 seconds: a pre-push hook for formatting, spelling, the guidance check and, for workflow changes, actionlint; and cargo-nextest installed so the local gate runs what CI runs | cargo-nextest is missing on the development host, so the gate falls back to `cargo test` | Not started |

Measured and decided against:

- **Optimizing the workspace's own crates in the test build.** At `opt-level` 1 the
  app's tests took 0.68 of today's time and its test target 2.4 times as long to compile
  (164 s against 69 s locally; levels 2 and 3 added compile time and no test gain). Each
  shard pays the compile and saves only its share of the tests, so on the hosted macOS
  job two shards would save about 315 s each for about 350 s more compiling. The profile
  stays; it is measured again only if one build ever feeds many shards.
- **Splitting by measured duration.** nextest's round-robin split is within 5 % of it.

## The owner's list

| Item | Why | Monthly cost | State |
| --- | --- | --- | --- |
| Give the development host's GitHub token the `workflow` scope (`gh auth refresh -h github.com -s workflow`) | GitHub refuses a push that changes a workflow file without it; every change above that touches `.github/workflows/` waits on it | $0 | Done by the owner on 2026-11-18 |
| Move the repository to a free organization | The only way to a merge queue, which would let a pull request run a narrower selection and the queue the full suite, and end the update-and-rerun cycle when several pull requests are open; it also makes the organization-only runner vendors installable | $0; the two owner-name conditions under `.github/`, the documentation's and the website's links and the package's source URL change with it, and caches seed again | Open, the owner's to decide; best decided before the first release, while no release asset or package points at the current address; changes 1 to 11 do not depend on it |

Not on the list, by the owner's decision of 2026-11-18: paid macOS runners (a vendor's 6
processor runner at $0.06 a minute would have been about $130 to $225 a month by
estimate) and bought cache storage ($0.07 a GB a month). The research notes keep the
prices as facts; the plan does not use them.

## Totals

| | Before (2026-11-17) | After |
| --- | --- | --- |
| Full pull-request run, wall-clock, median / p95 | 42 / 54 min | 28 min 44 s, 29 min 11 s and 29 min 42 s in the first three sharded runs (2026-11-19, with the Ubuntu cache missing in five of six shards); 35 min 12 s (27 min 16 s without a wait behind another run's macOS jobs), 32 min 06 s and 34 min 47 s in the three runs after the Ubuntu key, the last with two Ubuntu shards cold on a new image; 42 min 08 s and 41 min 37 s in the two runs that the helper's change made cold on every lane (#221's own and `main`'s on its merge), and 28 min 50 s in the run after them, with one Ubuntu shard cold on the newer image. Median of the seven runs that no helper change had just made cold: 29 min 42 s. A run now waits for a macOS test shard (1,009 to 1,861 s in those three runs), so changes 4 and 5 are what shorten it |
| Runner time per full run, median | 146 min (63 macOS) | 185, 173 and 190 min (71, 74 and 79 macOS) in the first three sharded runs, against 205 min (74 macOS) for the unsharded run of the same day; 174, 171 and 203 min (66, 75 and 79 macOS) in the three runs after the Ubuntu key; 234 and 258 min (109 and 117 macOS) in the two runs cold on every lane, and 166 min (75 macOS) in the run after them |
| Runs a month | about 107 (150 in 42 days) | |
| Cost a month | $0 | $0 |
