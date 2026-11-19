# The Ubuntu `apt-get` step on GitHub-hosted runners (2026-11-19)

Research note for the [CI clean-up plan](ci-cleanup.md), change 9. Every source below was read on 2026-11-19 between about 08:40 and 09:10 UTC unless a row says otherwise. Research only: no tracked file was changed and nothing here was run on a hosted runner.

## Decision and scope

- **Question.** `.github/actions/setup-rust/action.yml` runs `sudo apt-get update` and `sudo apt-get install -y --no-install-recommends` of 19 names on `ubuntu-24.04` and `ubuntu-24.04-arm`. Since 2026-11-18 about 20:00 UTC the step takes 61 to 718 s on AWS-hosted x64 runners. Which current, primary-source facts bear on five candidate remedies: (a) not installing apt's `cmake`, (b) installing nothing the image already holds, (c) another mirror, (d) keeping the `.deb` files in the Actions cache, (e) narrowing `apt-get update`?
- **Recommendation.** None is made; the plan chooses. The facts force only these four statements:
  1. Nobody has published this slowdown as an incident, so there is no declared cause, end time or resolution to plan around.
  2. apt moves to the next mirror only when a fetch fails. A mirror that answers slowly is never abandoned, and the image's `Acquire` settings cover stalls only.
  3. apt accepts a `.deb` already in `/var/cache/apt/archives` on a size match alone. Restoring files there does not keep apt's signature chain unless something else checks their hashes against the signed index.
  4. Names the image already holds cost no archive bytes today; apt fetches only the 36 new packages.
- **Outside this decision.** The Rust cache key design, the Arch container job, macOS, and anything bought. Retries and wider timeouts are excluded by the owner; the workarounds posted in the upstream issue thread are of that kind and are not candidates.

## Evidence

### Local measurements (not re-measured here; the tool, its table and the logs are kept locally, not versioned)

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| `.local/ci-cleanup-2026-11-17/analysis/apt_step.py` over 70 retained job logs of 18 runs, table `.local/ci-cleanup-2026-11-17/apt/apt_step-2026-11-19-0900.tsv` | Logs from 2026-11-16 00:38 to 2026-11-19 06:26 UTC | x64 AWS: 28 to 46 s before 2026-11-18 20:00 UTC (n=11), 61 to 718 s after (n=15). x64 Azure: 12 to 27 s before (n=20), 12 to 290 s after (n=9). arm64: 15 to 35 s (n=15). Install fetches 19.2 MB on x64, 18.1 MB on arm64, 36 packages. The build selects `/usr/local/bin/cmake` 3.31.6 on every hosted image. | One repository's runs; no run after 06:26 UTC. |
| Sample logs of run 44472062033 (`.local/ci-cleanup-2026-11-17/shards/44472062033/`), read with `sed`, `grep` and `awk` on 2026-11-19 | Jobs started 2026-11-19 06:26 UTC | AWS job: `Region: us-west-2`, `Cloud: AWS`, image 20261111.127.1, `Mirrorlist [152 B]`, archives from `us-east-2.ec2.archive.ubuntu.com`, the `noble-security` pocket fetched directly from `http://security.ubuntu.com` after a 30.4 s gap and an `Ign:` line; four archives show `Ign:` twice on the EC2 mirror and then `Get:` from `https://archive.ubuntu.com`. Azure job: `Region: westus2`, image 20261111.370.1, `Mirrorlist [144 B]`, all four pockets from `azure.archive.ubuntu.com`. arm64 job: `Region: centralus`, `Cloud: Azure`, `ports.ubuntu.com`, no mirror-list line. | Three jobs of one run. |
| The same logs, `apt-get update` `Get:` lines summed by `awk` | Same | AWS x64 update, 13,202,052 B in total: `us-east-2.ec2.archive.ubuntu.com` 6,421,600; `security.ubuntu.com` 5,259,700; `packages.microsoft.com` 1,520,600 (amd64 `Packages` 794,000 and arm64 `Packages` 723,000). By kind: `Packages` 10,796,000; `Translation-en` 2,024,300; `InRelease` 381,600. By Ubuntu component: main `Packages` 2,654,000; universe `Packages` 2,946,000; restricted `Packages` 3,679,000 and `Translation` 847,000; multiverse `Translation` 27,300. The 36 archives came from `noble/main` (25), `noble/universe` (3) and `noble-updates/main` (8). arm64 update: 15,516,100 B, restricted `Packages` 5,536,000. | apt's rounded bracket sizes; one sample per platform. |
| The same AWS log, `apt-get install` section | Same | 8 of the 19 requested names print "is already the newest version" (`pkg-config`, `git`, `openssh-client`, `ca-certificates`, `libssl-dev`, `libzstd-dev`, `python3`, `xdg-utils`); 11 requested and 25 dependency packages are new. `build-essential` is 4,928 B and `clang` 5,846 B. | One AWS image version. |

### 1. Is there a known degradation?

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| GitHub status, <https://www.githubstatus.com/api/v2/incidents.json> and `summary.json` (history at <https://www.githubstatus.com/history>) | 09:04 UTC; page updated 2026-11-19T08:04:40Z | "All Systems Operational", no open incident, no component degraded. The newest incident is a Copilot one of 2026-11-18 09:00 to 09:34 UTC. Nothing for Actions or hosted runners from 2026-11-18 20:00 UTC on. | Lists only what GitHub declares. |
| Canonical status, <https://status.canonical.com/> (its public summary JSON and <https://status.canonical.com/rss>) | 09:04 UTC; components last polled 08:10 to 08:12 UTC | No active incident and none dated 2026-11-19. The only incident of 2026-11-18 is `gopkg.in`, 13:58 to 14:10 UTC. `archive.ubuntu.com`, `Azure archive mirrors` and `security.ubuntu.com` are Operational; their last status changes are 2026-10-07, 2026-10-27 20:07 UTC and 2026-11-10. | The page has no component for the EC2 regional mirrors or `ports.ubuntu.com`. The components are URL monitors: they show reachability, not transfer speed. |
| `actions/runner-images` issue [#14823](https://github.com/actions/runner-images/issues/14823), open since 2026-09-30 | 19 comments, last 2026-11-19T04:22Z | The only issue reporting slow mirrors among those updated since 2026-11-10 (three searches of the repository). Three user comments report slow `us-east-2.ec2.archive.ubuntu.com` on AWS-hosted runners: [2026-11-18T14:22Z](https://github.com/actions/runner-images/issues/14823#issuecomment-6981335195), [15:05Z](https://github.com/actions/runner-images/issues/14823#issuecomment-6982456332) and [2026-11-19T04:22Z](https://github.com/actions/runner-images/issues/14823#issuecomment-6999616982) (about 110 kB/s). The last maintainer or staff comment is of 2026-10-27. | User reports, not a statement by GitHub or Canonical. The first report predates the local step change by about six hours. |
| Same issue, staff comments of [2026-10-27T19:52Z](https://github.com/actions/runner-images/issues/14823#issuecomment-6452056947) and [20:52Z](https://github.com/actions/runner-images/issues/14823#issuecomment-6453261284) | Same | Precedent: for the Azure mirror slowdown of 2026-10-26/27 GitHub said the fault was "the mirror itself and not Actions runners", Canonical posted it on its status page, and it was declared healthy at 20:52 UTC that day. | A different mirror and a different event. |
| Open [Announcement](https://github.com/actions/runner-images/labels/Announcement) issues | 11 open | None concerns apt mirrors, AWS-hosted runners or CMake. | |

**Answer.** Nothing is published. From 2026-11-18 20:00 UTC to 2026-11-19 09:04 UTC neither Canonical, GitHub's status page nor a `runner-images` maintainer reported slow or failing Ubuntu mirrors. There are user reports in an open issue, unanswered. Nothing is declared, so nothing is declared resolved.

### 2. Runners on AWS

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| GitHub Changelog, [2026-10-28, "GitHub Actions adds multi-cloud compute to improve reliability"](https://github.blog/changelog/2026-10-28-github-actions-adds-multi-cloud-compute-to-improve-reliability/) | Modified 2026-11-03 | "eligible Linux jobs may run on Amazon Web Services (AWS) in addition to Microsoft Azure"; applies to standard 2-core and 4-core Linux runners; automatic on Free, Pro and Team; "Your existing runner labels and workflow syntax remain unchanged." Its one recommended action is about IP allowlists. | Says nothing of image contents, apt mirrors, regions or speed. |
| GitHub Docs source, [`content/actions/concepts/runners/github-hosted-runners.md`](https://github.com/github/docs/blob/39c1e014c75e972ab1fe531a81bc5bd42b543b07/content/actions/concepts/runners/github-hosted-runners.md) lines 98 to 111, and [`content/actions/reference/runners/github-hosted-runners.md`](https://github.com/github/docs/blob/39c1e014c75e972ab1fe531a81bc5bd42b543b07/content/actions/reference/runners/github-hosted-runners.md) lines 84 to 94 | `github/docs` 39c1e014, committed 2026-11-18 | Section "Cloud hosts used by GitHub-hosted runners": "These jobs can run on either Amazon Web Services (AWS) or Microsoft Azure"; "Jobs are automatically scheduled across available cloud providers." It warns that network allowlists and Azure SDK credentials can fail on the other cloud. | Page source, not the rendered page. No guarantee of equal images, mirrors or performance is stated; that section documents no opt-out for these plans. |
| `actions/runner-images` Git refs and releases | Refs API | Tags `ubuntu24/20261111.370` (fb099341), `ubuntu24/20261116.376` (27322eb5) and `ubuntu24-arm64/20261111.182` (6573a3f3) exist. `ubuntu24/20261111.127` and `ubuntu24/20261116.133`, the versions the AWS jobs print, return 404, so the log's "Included Software" and "Image Release" links are dead. Issue [#15341](https://github.com/actions/runner-images/issues/15341) (open, 2026-11-17) reports it; the one reply promises a look. | |
| PR [#15335](https://github.com/actions/runner-images/pull/15335), merged 2026-11-17T10:15Z as [f2f91839](https://github.com/actions/runner-images/commit/f2f91839f985dcbde42542557c72de8d0c2970a6) | PR body and file list | "Add AWS as an optional build target for Ubuntu 24.04 and 26.04 x64 images while keeping Azure as the default provider", with "the AWS us-east-2 APT mirror". | Merged six days after image 20261111.127 was built, so the public repository did not hold that image's build scripts at any tag. |
| Issue [#15347](https://github.com/actions/runner-images/issues/15347), open, 2026-11-17 | Body | Another AWS-only image difference reported by a user (`/etc/hosts`). | User report. |

**Answer.** The changelog entry and the docs section are the only documents; both describe scheduling and network addresses, and neither promises that an AWS-hosted runner matches an Azure-hosted one. The AWS image versions have no tag, release or published software list.

### 3. What the hosted image guarantees about CMake

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| [`images/ubuntu/scripts/build/install-cmake.sh`](https://github.com/actions/runner-images/blob/27322eb5268668e27e64007fdd1353ec7387dc0b/images/ubuntu/scripts/build/install-cmake.sh) | Tag `ubuntu24/20261116.376`; the same lines at 6573a3f3 (arm64 tag) | Downloads Kitware's release installer for `x86_64` or `aarch64`, compares it with the release's SHA-256 file, and runs it with `--prefix=/usr/local --exclude-subdir`: the image's CMake is `/usr/local/bin/cmake`, not an apt package. | |
| [`images/ubuntu/toolsets/toolset-2404.json`](https://github.com/actions/runner-images/blob/27322eb5268668e27e64007fdd1353ec7387dc0b/images/ubuntu/toolsets/toolset-2404.json) lines 325 to 332; [`toolset-2404-arm64.json`](https://github.com/actions/runner-images/blob/6573a3f3872c75e7d109ddc601df33cad111c9ab/images/ubuntu/toolsets/toolset-2404-arm64.json) | Same tags | `"cmake": {"version": "3.31.6", "pinnedDetails": {"reason": "Pinned to avoid CMake 4.0 compatibility issues", "link": "https://github.com/actions/runner-images/issues/11926", "review-at": "2026-12-01"}}` on both architectures. | A pin with a review date twelve days away. |
| [`Ubuntu2404-Readme.md`](https://github.com/actions/runner-images/blob/27322eb5268668e27e64007fdd1353ec7387dc0b/images/ubuntu/Ubuntu2404-Readme.md) line 71 (line 73 at fb099341); [`Ubuntu2404-Arm64-Readme.md`](https://github.com/actions/runner-images/blob/6573a3f3872c75e7d109ddc601df33cad111c9ab/images/ubuntu/Ubuntu2404-Arm64-Readme.md) line 70 | Same tags | "CMake 3.31.6" under "Tools" on x64 and arm64: the software list names it as part of the image. | No list exists for the AWS image versions; there the fact rests on the local measurement alone. |
| PR [#15344](https://github.com/actions/runner-images/pull/15344) (open), issue [#15331](https://github.com/actions/runner-images/issues/15331) "Update CMake 4.5" (open) | 2026-11-17, 2026-11-15 | The open PR moves the pin's `review-at` to 2027-03-01 for 24.04 x64 and arm64. The open request asks for CMake 4.5. | Neither is decided. |
| [`README.md`](https://github.com/actions/runner-images/blob/fbb26cb01c3651ea3a0347e54bfa2ef83d740d49/README.md), "Image Releases", "Support Policy", "Default Version Update Policy" | `main` at fbb26cb0 | "We typically deploy weekly updates to the software on the runner images." "In general, once a new version is installed on the image, we announce the default version update 2 weeks prior to deploying it"; up to 1 month "for potentially dangerous updates". Tools are "typically" removed 6 months after deprecation. Notable changes carry the Announcement label. | "Typically" and "in general": a stated practice, not a guarantee of a version. |
| Docs source, [`concepts/runners/github-hosted-runners.md`](https://github.com/github/docs/blob/39c1e014c75e972ab1fe531a81bc5bd42b543b07/content/actions/concepts/runners/github-hosted-runners.md) lines 59 to 80 | 39c1e014 | Tools "are updated weekly"; GitHub recommends actions to select tool versions because that "ensures the tool versions used in your workflow will remain the same regardless of software updates". | |

**Answer.** The image's CMake is a Kitware binary at `/usr/local/bin`, named in the published list for x64 and arm64 and pinned to 3.31.6 with a review due 2026-12-01. Its presence is documented; its version is promised only up to an announcement about two weeks ahead of a change.

### 4. What the hosted image does about apt mirrors

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| [`images/ubuntu/scripts/build/configure-apt-sources.sh`](https://github.com/actions/runner-images/blob/27322eb5268668e27e64007fdd1353ec7387dc0b/images/ubuntu/scripts/build/configure-apt-sources.sh) lines 16 to 33 | Tag `ubuntu24/20261116.376` | Writes `/etc/apt/apt-mirrors.txt`: the cloud's mirror with `priority:1` (`http://azure.archive.ubuntu.com/ubuntu/`, or `http://us-east-2.ec2.archive.ubuntu.com/ubuntu/` when the base image's sources name it), then `https://archive.ubuntu.com/ubuntu/` `priority:2` and `https://security.ubuntu.com/ubuntu/` `priority:3`. It rewrites those hosts in `/etc/apt/sources.list.d/ubuntu.sources` to `mirror+file:/etc/apt/apt-mirrors.txt` and copies the file over cloud-init's template. `ports.ubuntu.com` is not matched, so arm64 24.04 has no mirror list. | The first entry is plain `http`. |
| The same file at [fb099341](https://github.com/actions/runner-images/blob/fb099341ddaf44a428fb17495341cb7c2300faf2/images/ubuntu/scripts/build/configure-apt-sources.sh) (tag `ubuntu24/20261111.370`) | | No AWS branch: Azure entry plus the same two fallbacks. The three lines total 144 B (`printf | wc -c`), the size the Azure logs print; the AWS variant totals 152 B, the size the AWS logs print. | The AWS list's content is inferred from its size and the hosts used, since the AWS images have no tag. |
| Same script, lines 20 to 21, with the local AWS log | | The AWS mirror is the fixed host `us-east-2`; the sampled AWS job ran in `us-west-2`. The script would also route `security.ubuntu.com` through the list, yet the AWS log fetches `noble-security` from `http://security.ubuntu.com` directly, with no alternate. | Why the AWS image differs from the public script is not established. |
| [`images/ubuntu/scripts/build/configure-apt.sh`](https://github.com/actions/runner-images/blob/27322eb5268668e27e64007fdd1353ec7387dc0b/images/ubuntu/scripts/build/configure-apt.sh) lines 17 to 68 | Same tag; last changed 2026-09-29 | `/etc/apt/apt.conf.d/zz-retries`: `Acquire::Retries "1"`, `Acquire::http::Timeout "15"`, `Acquire::https::Timeout "15"` on x64; arm64 24.04 keeps 3 and 30 because it "is served by ports.ubuntu.com, which the mirror list does not cover". Also `90assumeyes`, `99-phased-updates` (always include phased updates), `90-index-targets` (DEP-11 off, "7.6 MB from every apt-get update"), and `99bad_proxy`: `Pipeline-Depth 0`, `No-Cache true`, `BrokenProxy true`. | Issue [#15174](https://github.com/actions/runner-images/issues/15174) (open) questions `No-Cache`. |
| apt `apt-transport-mirror(1)`, [`doc/apt-transport-mirror.1.xml`](https://salsa.debian.org/apt-team/apt/-/blob/4dde455f5e05fdc180bd677561540b7ba9cc198d/doc/apt-transport-mirror.1.xml) | Tag 2.8.4 (4dde455f) | "If the acquisition of a file via a mirror fails, the method ensures that another possible mirror from the list is automatically tried until either the file is retrieved or no mirror is left". "The mirrors with the lowest number are tried first"; equal priorities are chosen at random. "This transport has no configuration options at present." Security depends on the transports used for the list and for the mirrors. | Fallback is defined on failure only. |
| apt [`apt-pkg/acquire-worker.cc`](https://salsa.debian.org/apt-team/apt/-/blob/4dde455f5e05fdc180bd677561540b7ba9cc198d/apt-pkg/acquire-worker.cc#L639-667) lines 639 to 667; `apt-transport-http(1)` | Tag 2.8.4 | A transient failure spends the item's retries on the same URI first and only then takes the next alternative URI. `Acquire::http::Timeout` "applies to the connection as well as the data timeout". The manuals document a maximum rate (`Dl-Limit`) and no minimum rate. | Matches the sample: two `Ign:` on the EC2 mirror, then `archive.ubuntu.com`. |
| Launchpad, published sources of `apt` in noble | API read | noble-updates and noble-security carry `2.8.4ubuntu0.1` (published 2026-11-05 and 2026-11-04). | The version installed on the images was not read; Ubuntu's delta to 2.8.4 was not read. |

**Answer.** `configure-apt-sources.sh` writes the list and the sources; `configure-apt.sh` sets one retry and 15 s timeouts on x64. apt tries the lowest priority number first and moves on only after a failed fetch and its retries. A mirror serving at 30 kB/s never fails, so the two Canonical hosts already in the list are not used.

### 5. What apt does with a `.deb` already in `/var/cache/apt/archives`

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| apt [`apt-pkg/acquire-item.cc`](https://salsa.debian.org/apt-team/apt/-/blob/4dde455f5e05fdc180bd677561540b7ba9cc198d/apt-pkg/acquire-item.cc#L3494-3512) lines 3494 to 3512 (`pkgAcqArchive`) | Tag 2.8.4; identical at 2.8.3 | If a file of the package's name exists in `Dir::Cache::Archives` and its size equals the index's size, the item is marked `Complete`, `Local`, `StatDone` and is never queued for download. On a size mismatch the file is removed and fetched again. | |
| The 2.8.4 source tree, `grep` for `VerifyFile(` in `apt-pkg`, `apt-private`, `cmdline` | Tag archive | No call recomputes a cached binary archive's hashes on the install path; the call sites cover index files, `apt-get source`, CD-ROM copies and local `.deb` sources. | A search, not an execution; not tested on a runner. |
| apt [`methods/basehttp.cc`](https://salsa.debian.org/apt-team/apt/-/blob/4dde455f5e05fdc180bd677561540b7ba9cc198d/methods/basehttp.cc#L447-476) lines 447 to 476 | Tag 2.8.4 | A file in `archives/partial/` is resumed with a range request; on HTTP 416 the method hashes the local file and accepts it only if it equals the expected hashes. | Still one request per file to the mirror. |
| `apt-get(8)` and `apt-secure(8)` at 2.8.4 | | `--no-download` uses "only the .debs it has already downloaded"; `--print-uris` prints for each archive "the path, the destination file name, the size and the expected MD5 hash"; `clean` empties `archives/` and `archives/partial/`. `apt-secure(8)`: the Release file's signature covers the `Packages` checksums, which cover each package, and APT compares them for what it downloads. | The manuals do not say cached archives are re-verified. |

**Answer.** No. For a file already in `archives/`, `apt-get install` compares the size only, and re-downloads only on a size mismatch. Hashes are checked when apt downloads a file, or completes one from `partial/`.

### 6. What `apt-get update` must fetch

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| Local logs and [`install-ms-repos.sh`](https://github.com/actions/runner-images/blob/27322eb5268668e27e64007fdd1353ec7387dc0b/images/ubuntu/scripts/build/install-ms-repos.sh) | Tag `ubuntu24/20261116.376` | At run time the images answer from two sources: Ubuntu's pockets (`noble` is a `Hit`; `-updates`, `-backports`, `-security` are fetched; components main, universe, restricted, multiverse) and `packages.microsoft.com/ubuntu/24.04/prod`, added by `packages-microsoft-prod.deb`, which serves both amd64 and arm64 `Packages` on either architecture. | Other build scripts add apt lists during the build; whether each removes its list was not read script by script. The logs show only these two. |
| `apt-get(8)`, [`doc/apt.ent`](https://salsa.debian.org/apt-team/apt/-/blob/4dde455f5e05fdc180bd677561540b7ba9cc198d/doc/apt.ent), [`doc/examples/configure-index`](https://salsa.debian.org/apt-team/apt/-/blob/4dde455f5e05fdc180bd677561540b7ba9cc198d/doc/examples/configure-index) lines 449 to 450 | Tag 2.8.4 | `-o Foo::Bar=bar` sets any option. `Dir::Etc::SourceList` names the sources file and `Dir::Etc::SourceParts` the fragment directory. `--no-list-cleanup` (`APT::Get::List-Cleanup`): with cleanup on, apt erases list files it considers obsolete; "The only reason to turn it off is if you frequently change your sources list." | The effect of combining them on these images was not run. |
| `apt.conf(5)` and `sources.list(5)` at 2.8.4 | | `Acquire::Languages=none` forces "no Translation file". Per source, `Languages`, `Targets` and `Architectures` limit what is fetched; components are chosen per source. | |
| `apt.conf(5)`, `apt-secure(8)` | | Signature and freshness checks are separate options (`Check-Valid-Until`, `Trusted`, `Signed-By`); none of the narrowing options above touches them. | |

**Answer.** Ubuntu's pockets and Microsoft's repository. The documented narrowing options are `Dir::Etc::SourceList`, `Dir::Etc::SourceParts`, `APT::Get::List-Cleanup`, `Acquire::Languages` and the per-source `Languages`, `Targets`, `Architectures` and components. Each fetched index is still verified against its signed `InRelease`.

### 7. GitHub's own guidance

| Source | Checked | Finding | Limit |
| --- | --- | --- | --- |
| Docs source, ["Customizing GitHub-hosted runners"](https://github.com/github/docs/blob/39c1e014c75e972ab1fe531a81bc5bd42b543b07/content/actions/how-tos/manage-runners/github-hosted-runners/customize-runners.md) | 39c1e014 | The Ubuntu example is `sudo apt-get update` then `sudo apt-get install jq`, with the note "Always run `sudo apt-get update` before installing a package. In case the `apt` index is stale, this command fetches and re-indexes any available packages, which helps prevent package installation failures." | Nothing on mirrors, speed or caching packages. |
| Docs source, ["Dependency caching reference"](https://github.com/github/docs/blob/39c1e014c75e972ab1fe531a81bc5bd42b543b07/content/actions/reference/workflows-and-actions/dependency-caching.md) lines 69, 227 to 249 and 353 to 366, with reusables `cache-eviction-policy.md` and `cache-eviction-process.md` | 39c1e014 | Entries not accessed for 7 days are removed. No limit on the number of entries; the default total is 10 GB per repository. Over the limit GitHub saves the new entry and evicts by last access, oldest first, which "may cause cache thrashing". 200 uploads and 1500 downloads per minute per repository. A run restores from its own branch or the default branch; a pull request's cache is scoped to its merge ref. "Anyone with read access can create a pull request on a repository and access the contents of a cache." The package-manager table covers npm, pip, Gradle and Maven, RubyGems, Go and NuGet; apt is not in it. | No apt-specific guidance exists in these pages. |

## Compatibility and alternatives

Versions in play: Ubuntu 24.04.5; images 20261111.370.1 and 20261116.376.1 (Azure x64), 20261111.127.1 and 20261116.133.1 (AWS x64, untagged), 20261111.182.1 (arm64); runner 2.338.2; apt 2.8.4 upstream, `2.8.4ubuntu0.1` in noble-updates; apt's `cmake` 3.28.3-1build7; the image's CMake 3.31.6.

The byte figures are apt's own numbers from the tool's table and the sample logs. None is a time estimate; the time each candidate would save is unknown.

**(a) Not installing apt's `cmake` where the image's CMake is selected.**
- Facts: section 3; the build already selects `/usr/local/bin/cmake` on every hosted image. In noble's package index ([`noble/main` `Packages.xz`](http://archive.ubuntu.com/ubuntu/dists/noble/main/binary-amd64/Packages.xz), fetched without checking its signature) `cmake` depends on `cmake-data`, `libjsoncpp25` and `librhash0`, and none of the other 32 new packages depends on those three (checked against `noble/main`, `noble/universe` and `noble-updates/main`).
- Bytes: `cmake` 11,190,006 and `cmake-data` 2,154,936, together 13,344,942 B of the 19.2 MB x64 install; with `libjsoncpp25` (82,754) and `librhash0` (128,748), 13,556,444 B and 4 of 36 packages.
- Unknown: the arm64 sizes; whether the image's CMake stays below 4.x after the 2026-12-01 review; whether any build script needs `/usr/bin/cmake` or apt's CMake modules; what the AWS images document, since they publish no list. The Ubuntu cache key reads the dependency closure of twelve packages including `cmake`, so its input changes.

**(b) Installing nothing the image already holds.**
- Facts: apt already skips the 8 names it holds; they cost no archive bytes. All 36 fetched packages are absent from the image.
- Bytes: 0 for the 8 names. `build-essential` and `clang` are meta-packages of 10,774 B together.
- Unknown: whether the image supplies the commands those two meta-packages provide without them; the held set can change with each weekly image and is unpublished for the AWS images.

**(c) Another mirror.**
- Facts: section 4. The image's list already holds `https://archive.ubuntu.com/ubuntu/` and `https://security.ubuntu.com/ubuntu/`; apt reaches them only after a failure. Package integrity rests on the archive's signing key whichever host serves the bytes (`apt-secure(8)`).
- What a change would trust: reordering or removing entries of `/etc/apt/apt-mirrors.txt` trusts no new party for integrity, and changes which Canonical host serves, over `https` instead of `http`, and whose availability the step depends on. A host outside that list adds a new network party for availability, freshness and request metadata. A security reviewer decides.
- Bytes: none removed; the same bytes come from another party.
- Unknown: the speed of `archive.ubuntu.com` from either cloud at any moment (apt prints one overall rate); in the 06:26 UTC run one Azure job took 33 of 36 archives from it after failures on the Azure mirror, and its 19.2 MB took 4 min 38 s overall with `cmake` and `cmake-data` still served by the Azure mirror.

**(d) Keeping the `.deb` files in the Actions cache.**
- Facts: sections 5 and 7. Restored into `archives/`, the files are accepted on size alone; the chain then rests on the cache's write scope unless a separate step compares each file with the index's hash. The index still comes from `apt-get update`. The repository's 10 GB cache is full and only pushes to `main` save, so a new entry competes under last-access eviction. The files are public data.
- Bytes: up to 19.2 MB (x64) or 18.1 MB (arm64) no longer fetched from a mirror; they are fetched from the cache service instead. The update's 12.9 to 15.5 MB remain.
- Unknown: the entry's stored size; restore time and speed on AWS-hosted runners; how often the 36 versions change (8 came from `noble-updates`), which sets the miss rate.

**(e) Narrowing `apt-get update`.**
- Facts: section 6. The install drew only on main and universe of `noble` and `noble-updates`. The image already drops DEP-11. GitHub's guidance is to run `apt-get update` before every install.
- Bytes, AWS x64 sample of 13,202,052 B: `Translation-en` 2,024,300; `packages.microsoft.com` 1,520,600; restricted and multiverse 4,553,300 (874,300 of it also counted under `Translation-en`). All lines other than main and universe `Packages` and Ubuntu's `InRelease` files sum to 7,223,900 B.
- Unknown: the measured effect of each option on these images; time saved, since in the sample 30.4 s of the 49 s update was a wait on `security.ubuntu.com`, a host that narrowing keeps.

Evidence that would change this picture: a Canonical or GitHub statement on the EC2 mirror; a tag or published list for the AWS images; a change to the CMake pin; a merged fix for #14823 or #15174.

## Enforcement and acceptance

- Owners and contracts: `.github/actions/setup-rust/action.yml` and its helper; `docs/ci.md`, "Ubuntu native key". A mirror change or a third-party action goes to the security reviewer under `docs/development/security-review.md`.
- Existing evidence: the table and tool named above. No candidate has been run; any claim of time saved needs the same tool over new runs on both clouds and arm64.
- Next checks that would resolve an unknown: print `apt-config dump Acquire`, `/etc/apt/apt-mirrors.txt`, the files under `/etc/apt/sources.list.d/` and `apt --version` once on an AWS-hosted, an Azure-hosted and an arm64 job; record the cache entry size and restore time for (d); read the arm64 `cmake` size from the `ubuntu-ports` index.

## Outcome

None yet. Research only; no decision adopted and no code changed.

## Unverified or unknown

- The cause of the slowdown, and whether it has ended: unpublished.
- The build scripts, mirror list text, apt options and software list of the AWS image versions 20261111.127.1 and 20261116.133.1: no tag exists. The list's content is inferred from its 152 B size.
- Why the AWS image fetches `noble-security` directly from `security.ubuntu.com` when the public script would route it through the list.
- The apt version installed on the hosted images, and Ubuntu's changes in `2.8.4ubuntu0.1` to the code read at upstream 2.8.4.
- Whether a restored `.deb` is used unverified in practice on a runner: read from source, not executed.
- The speed of `archive.ubuntu.com` and of the Actions cache service from AWS-hosted runners.
- Whether the image's CMake remains 3.31.6 after 2026-12-01.
- Time saved by any candidate: unknown; only bytes are measured.
- The rendered documentation pages were not compared with their sources at 39c1e014.
