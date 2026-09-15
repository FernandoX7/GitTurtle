# GitTurtle 0.1.0 source preview — draft release notes

Draft binary-release copy for review; no public binary is announced. The
marketing site is published at [gitturtle.com](https://gitturtle.com).
The [Pages hostname](https://gitturtle.pages.dev) is also available.

This preview improves everyday local Git work, with more dependable refresh,
clearer history updates, and corrected split-diff scrolling.

## What changed

- **Automatic refresh handles large working trees more reliably.** Linux watch
  coverage follows Git ignore rules while retaining tracked files inside ignored
  directories. Essential Git metadata remains watched when broader coverage is
  constrained. Directory cleanup, renames and unrelated sibling directories no
  longer unnecessarily disable refresh. A compact status offers **Refresh** and
  **Details** when coverage needs attention, and clears after recovery.
- **New commits are easier to discover.** History follows new rows when you are
  already viewing the newest commits, with a restrained update cue. Reading
  older history, searching, or inspecting a comparison retains that context and
  offers **Show latest**. **Latest** reads current local history without changing
  the selected inspector. Counts appear only when established accurately;
  otherwise the message says **History updated**. Successful commits provide a
  **View commit** action while preserving a subsequent draft.
- **Split panes stay synchronized through scrolling bursts.** The editor now
  exposes accepted scroll positions immediately and lets newer wheel input
  supersede an older pending update. This corrects the observed pane bounce;
  it is not a claim of higher frame rate or a new text renderer.
- **Build information and Linux upgrades are clearer.** About shows the version,
  source revision, target and profile, and offers **Copy bug diagnostics**.
  `--version` and `--build-info` work without opening the app. Linux upgrades
  preserve settings, retain a recoverable previous installation, and refuse to
  replace a running installed executable.
- **The marketing site is published.** The standalone static site uses
  mint branding, real current app captures, and a manual keyboard-operable
  walkthrough. It links to source installation, documentation, support, GitHub
  and sponsorship, with reduced-motion styles and a JavaScript-free fallback.

Passive refresh remains local and read-only. Fetch, pull, push and repository
writes require explicit interaction.

## Try the preview

Use the [macOS source-build guide](user-guide.md#package-locally-on-macos) or the
[Ubuntu 24.04 x86-64 guide](linux.md#build-from-source-on-ubuntu-2404).
No public binary download is offered by this preview. Local macOS bundles are
ad-hoc signed, not notarized; Linux bundles have no automatic updater.
[Platform limits](linux.md#platform-limits) describe unavailable Linux features
and outstanding desktop coverage.

For a Linux upgrade, let Git operations finish, quit GitTurtle, and run the new
bundle's installer. The retained installer's `--rollback` restores the previous
payload while keeping settings and drafts. Follow the complete
[installation and recovery instructions](linux.md#install-a-local-preview-bundle).

Report problems through the [issue forms](https://github.com/FernandoX7/GitTurtle/issues/new/choose).
Include reproduction steps and About's **Copy bug diagnostics** output. Those
diagnostics contain build/display settings; they omit repository paths, source
text, branch names, accounts and credentials.

## Evidence and remaining release steps

The [refresh record](benchmarks/2026-09-14-refresh-reliability.md) documents
reproduction, bounds and locked workspace validation. The
[native investigation](benchmarks/2026-09-14-native-preview.md) identifies each
exercised build and distinguishes physical-desktop observations, virtual-display
captures and frame-measurement limits. The [website guide](../website/README.md)
records browser coverage and screenshot provenance. These records do not imply
validation on another platform or compositor.

Public Linux binaries remain blocked on the exact `mac` 0.1.1 and Rust `ufbx`
0.11.3 [notice questions](licenses/linux-notice-review.md); other targets need
their own notice review. Before publication, attach final executable/archive
identities and validation to the release, complete the
[release gates](public-launch.md#before-a-public-binary-release), and approve the
publication. Website publication was separately approved and completed on
Cloudflare Pages. The approved `gitturtle.com` binding and apex CNAME are in
place; Cloudflare reports the domain active and HTTPS verification passed. The
[deployment record](../website/README.md#cloudflare-pages-deployment) identifies
the published assets and validation limits. This does not clear or authorize
a binary release.
