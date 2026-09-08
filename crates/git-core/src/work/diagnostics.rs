//! Guidance supplements Git's original diagnostics; it never changes execution.
pub(super) fn guidance(stderr: &[u8], stdout: &[u8], creates_commit: bool) -> Option<&'static str> {
    let output = format!(
        "{}\n{}",
        String::from_utf8_lossy(stderr),
        String::from_utf8_lossy(stdout)
    )
    .to_lowercase();
    if output.contains("host key verification failed")
        || output.contains("remote host identification has changed")
    {
        Some(
            "Verify this SSH host and its fingerprint through your trusted administrator or host settings. Repair the known-host entry only after verification, then explicitly retry. GitTurtle respects your SSH configuration.",
        )
    } else if output.contains("permission denied (publickey")
        || output.contains("no supported authentication methods")
    {
        Some(
            "SSH did not accept an available key. Check the remote URL, configured SSH identity and agent, and your repository access. Unlock or load the intended key outside GitTurtle, then explicitly retry.",
        )
    } else if output.contains("credential-")
        && (output.contains("not a git command")
            || output.contains("not found")
            || output.contains("cannot run")
            || output.contains("no such file"))
    {
        Some(
            "The configured credential helper could not run. Check credential.helper and the helper's installation or executable path in your existing Git configuration. Complete any sign-in outside GitTurtle, then explicitly retry.",
        )
    } else if output.contains("authentication failed")
        || output.contains("terminal prompts disabled")
        || output.contains("could not read username")
        || output.contains("could not read password")
        || output.contains("http 401")
        || output.contains("http 403")
    {
        Some(
            "Check the remote URL and your access, then sign in through your configured Git credential helper outside GitTurtle. This app does not open terminal credential prompts. Once credentials are available, explicitly retry the operation.",
        )
    } else if output.contains("failed to sign")
        || output.contains("signing failed")
        || output.contains("gpg failed")
        || output.contains("could not open a connection to your authentication agent")
    {
        Some(
            "Git could not use the configured signing identity. Check user.signingkey, gpg.format and the configured signing program; unlock the intended key or agent outside GitTurtle. Signing remains enabled. Review the current staged work before explicitly retrying.",
        )
    } else if output.contains("conflict") || output.contains("fix conflicts") {
        Some(
            "Review the current operation and conflicted files in Changes. Resolve each file, then use Continue. Abort and Stop and keep files explain their effects before acting. No write is retried automatically.",
        )
    } else if output.contains("not possible to fast-forward") || output.contains("non-fast-forward")
    {
        Some(
            "The branch tips need review. An explicit Fetch updates local remote-tracking information; inspect the divergence and deliberately choose merge or rebase. Push remains non-forcing.",
        )
    } else if creates_commit {
        Some(
            "Review Git's output and the current repository state. For a commit, check the configured hooks, identity and signing program. GitTurtle preserves that configuration and does not bypass failures or retry automatically.",
        )
    } else {
        None
    }
}
