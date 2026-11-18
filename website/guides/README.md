# First-party guides

Edit the HTML fragments here and run `python3 website/build_guides.py` from the repository root. The renderer writes HTML documents under `website/public/`. `python3 website/build_guides.py --check` verifies that the committed pages match the sources. `python3 website/check.py` checks the published files and links.

Each fragment starts with two single-line comments holding its title and description, in that order. The template uses these plain-text values for the visible heading and introduction, document title, and sharing metadata. Author the rest as semantic HTML. Use root-relative paths for links, trailing slashes for page routes and full-size links for product captures. Give each screenshot link the `screenshot-link` class and a short `data-screenshot` title. The renderer includes the image viewer on those pages. `docs/history-and-diffs.html` produces `/docs/history-and-diffs/`, and `docs/index.html` produces `/docs/`. Sources under `reference/` omit that prefix in their public URL, so `reference/install/macos.html` produces `/install/macos/`. Update the navigation in the renderer when adding a destination.

## Content sources

| Guide | Repository source |
| --- | --- |
| First repository, History and diffs, Stage and commit | [User guide](../../docs/user-guide.md), [repository tabs](../../docs/repository-tabs.md) |
| Worktrees and recovery | [Worktrees and reflog](../../docs/parallel-work-recovery.md), [conflict blocks](../../docs/conflict-blocks.md), [interactive rebase](../../docs/interactive-rebase.md) |
| GitHub pull requests | [GitHub collaboration](../../docs/github-collaboration.md) |
| Previews and shortcuts | [File support matrix](../../docs/file-previews.md), [commands](../../docs/command-palette.md), [LFS previews](../../docs/lfs-previews.md) |
| Omarchy and themes | [Omarchy integration](../../README.md#first-class-omarchy-support), [application settings](../../docs/user-guide.md#open-a-project-and-work-with-git) |
| Installation, release status and performance | [Getting started](../../README.md#get-started), [platform status](../../README.md#platform-status), [Linux installation](../../docs/linux.md), [measurements](../../README.md#measured) |
| Support and legal | [Support](../../SUPPORT.md), [license](../../LICENSE), [third-party notices](../../THIRD_PARTY_NOTICES.md) |

The [asset manifest](../asset-provenance.json) identifies the native builds, themes and fixtures used in guide screenshots.
