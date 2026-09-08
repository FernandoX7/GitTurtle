# Tags and contextual ignore

These workflows are native GPUI dialogs available from an open repository. They
use the existing serialized operation executor, operation feedback and local
refresh. Neither browsing tags nor preparing an ignore rule starts a network
operation. The coordinating milestone record contains native interaction evidence
and the actual checked build identity.

## Tags

Open the current-branch picker and choose **Tags…**. The dialog lists local tags
with their name, lightweight/annotated kind and target object ID. Filter by name
or object ID; Return opens the first match and tag buttons participate in normal
keyboard focus traversal. The browser loads at most 10,000 tags alphabetically,
displays up to 100 matches at once, and states both boundaries explicitly.

Select a tag to inspect its object ID, target type/ID, tagger, and original
annotation, including any stored signature. Copy object ID is explicit. An
annotation above 256 KiB is unavailable with an error rather than an unbounded
editor. Signature text is inspection data; its display does not claim signature
verification.

**Create tag…** starts at the selected history commit, or HEAD when no commit is
selected. The target field may be edited. Choose Lightweight or Annotated; the
latter requires a message. Review resolves the target to an immutable commit ID
and shows the exact local consequence before Create tag. Tag naming and signing
configuration are checked again when the accepted write executes. Git's configured
identity and signing program remain in control. `tag.gpgSign=true` requires an
annotated tag; GitTurtle does not silently disable signing. A signing failure
retains the edited form for reopening and reports Git's diagnostics. Successful
creation clears only the matching retained form.

**Delete local tag…** reviews the captured name and object ID. Deletion compares
the captured object ID while holding Git's reference lock, so a moved tag is
refused. This removes only the local name; remote tags, branches, working files and
the index remain intact.

**Push to <remote>…** is an explicit network action for one named tag. Review
identifies the named remote and redacted destination URL. The backend refuses a
changed remote configuration, changed local tag, or multiple push URLs. It disables
mirror, force, follow-tags and submodule recursion and sends only the captured tag
refspec. It never replaces an existing different remote tag. Ordinary branch Push
keeps its previous branch-only behavior. Remote tag deletion is not exposed.

## Ignore from Working Changes

Select an untracked item and choose **Actions → Ignore selected untracked file…**,
or right-click its row and choose **Ignore…**. Tracked rows explain that Ignore
applies to untracked files. The form distinguishes **Shared .gitignore** from
**Local excludes** and optionally offers the selected file's containing directory.
The repository root cannot be selected as a directory rule.

**Preview rule** checks the current status and destination off the UI thread, then
shows the exact escaped rule and absolute destination before **Add ignore rule**.
Shared rules append to the worktree root's `.gitignore`; these changes remain
unstaged for review. Local rules append to Git's `info/exclude`; linked worktrees
share this common Git metadata. A directory review counts tracked paths and states
that they remain tracked. Rules never silently untrack, stage or delete content.
As with Git, nested shared rules can override a file rule, and local excludes have
lower precedence than `.gitignore` rules.

Rules are anchored to the repository root and escape literal wildcard characters,
backslashes, spaces and comment/negation characters. Directory rules end in `/`.
Non-UTF-8 bytes use an explicit `\xHH` representation in the preview; the actual
rule retains the original bytes. A path containing a line break cannot be expressed
as one ignore rule and is refused.

Existing content is kept byte for byte, including CRLF/LF style; a missing final
newline is separated before the appended rule. The 1 MiB size limit is explicit.
Preparation captures the original bytes, file identity/permissions and parent
directory identity. Execution rereads the selection and destination, opens every
directory without following symlinks, prepares a new file under an exclusive lock,
rechecks the original and atomically replaces it. Stale changes require a new
preview. This protects GitTurtle writes and catches external changes during review;
uncooperative external writers are not participants in GitTurtle's lock protocol.

## Behavioral evidence

`cargo test --locked -p gitturtle-core --test tags_ignore` uses only disposable
working repositories and local bare remotes. It checks captured tag commits,
annotation bytes, signing failures, changed signing configuration, moved-tag
deletion refusal, exact remote refs with hostile mirror/follow-tags settings,
changed remote URLs, literal ignore matching, CRLF and missing-newline preservation,
unchanged index/working bytes, tracked directory contents, stale file identities,
symbolic destination/parent refusal and line-break refusal.

The raw non-UTF-8 working-filename fixture is Linux-only because APFS rejects those
filenames. Local fixture coverage establishes neither hosted-provider credentials
nor an unrun operating system. Native keyboard, focus, themes and packaged-app
checks are recorded separately in the milestone validation evidence.
