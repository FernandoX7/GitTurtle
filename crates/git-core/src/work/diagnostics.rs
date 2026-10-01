//! Guidance supplements Git's original diagnostics; it never changes execution.
/// A compact outcome headline only for the explicit fast-forward-only Pull.
/// Match Git's terminal refusal line, not advisory text or a remote/path name.
pub(super) fn pull_refusal_headline(
    stderr: &[u8],
    stdout: &[u8],
    fast_forward_pull: bool,
) -> Option<&'static str> {
    if !fast_forward_pull {
        return None;
    }
    [stderr, stdout]
        .into_iter()
        .any(|stream| {
            String::from_utf8_lossy(stream).lines().any(|line| {
                line.trim()
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case("fatal: Not possible to fast-forward, aborting")
            })
        })
        .then_some("Branches have diverged; choose Merge or Rebase to continue.")
}

// Keychain and osxkeychain exist only on macOS; elsewhere the same guidance
// leaves them unnamed.
const SSH_KEY_GUIDANCE: &str = if cfg!(target_os = "macos") {
    "SSH did not accept an available key. Check the remote URL, configured SSH identity and agent, and your repository access. Respond to the configured agent or passphrase prompt, or load the intended key with ssh-add; macOS SSH can use Keychain when configured with UseKeychain and AddKeysToAgent. Then explicitly retry."
} else {
    "SSH did not accept an available key. Check the remote URL, configured SSH identity and agent, and your repository access. Respond to the configured agent or passphrase prompt, or load the intended key with ssh-add. Then explicitly retry."
};
const CREDENTIAL_GUIDANCE: &str = if cfg!(target_os = "macos") {
    "Credentials may be expired or lack repository access. Check the remote URL and account permissions, then sign in with your configured Git credential helper (for example Git Credential Manager or macOS osxkeychain). GitTurtle can prompt when Git requests a username, token, or passphrase during an explicit operation. Refresh or remove only the expired credential through your helper, then explicitly retry."
} else {
    "Credentials may be expired or lack repository access. Check the remote URL and account permissions, then sign in with your configured Git credential helper (for example Git Credential Manager or a Secret Service helper). GitTurtle can prompt when Git requests a username, token, or passphrase during an explicit operation. Refresh or remove only the expired credential through your helper, then explicitly retry."
};

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
        Some(SSH_KEY_GUIDANCE)
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
        Some(CREDENTIAL_GUIDANCE)
    } else if output.contains("failed to sign")
        || output.contains("signing failed")
        || output.contains("gpg failed")
        || output.contains("could not open a connection to your authentication agent")
    {
        Some(
            "Git could not use the configured signing identity. Check user.signingkey, gpg.format and the configured signing program; unlock the intended key or agent outside GitTurtle. Signing remains enabled. Complete the configured signing program or pinentry prompt, then review the current staged work or tag before explicitly retrying.",
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

#[cfg(test)]
mod tests {
    use super::{guidance, pull_refusal_headline};

    /// The macOS wording, unchanged since these diagnostics first named macOS stores.
    const MACOS_SSH_KEY_GUIDANCE: &str = "SSH did not accept an available key. Check the remote URL, configured SSH identity and agent, and your repository access. Respond to the configured agent or passphrase prompt, or load the intended key with ssh-add; macOS SSH can use Keychain when configured with UseKeychain and AddKeysToAgent. Then explicitly retry.";
    const MACOS_CREDENTIAL_GUIDANCE: &str = "Credentials may be expired or lack repository access. Check the remote URL and account permissions, then sign in with your configured Git credential helper (for example Git Credential Manager or macOS osxkeychain). GitTurtle can prompt when Git requests a username, token, or passphrase during an explicit operation. Refresh or remove only the expired credential through your helper, then explicitly retry.";
    /// The owner's wording for every other platform.
    const OTHER_SSH_KEY_GUIDANCE: &str = "SSH did not accept an available key. Check the remote URL, configured SSH identity and agent, and your repository access. Respond to the configured agent or passphrase prompt, or load the intended key with ssh-add. Then explicitly retry.";
    const OTHER_CREDENTIAL_GUIDANCE: &str = "Credentials may be expired or lack repository access. Check the remote URL and account permissions, then sign in with your configured Git credential helper (for example Git Credential Manager or a Secret Service helper). GitTurtle can prompt when Git requests a username, token, or passphrase during an explicit operation. Refresh or remove only the expired credential through your helper, then explicitly retry.";

    #[test]
    fn authentication_guidance_names_macos_stores_only_on_macos() {
        let ssh = guidance(
            b"git@example.invalid: Permission denied (publickey).\nfatal: Could not read from remote repository.\n",
            b"",
            false,
        )
        .unwrap();
        let http = guidance(
            b"remote: Invalid username or password.\nfatal: Authentication failed for 'https://example.invalid/repo.git/'\n",
            b"",
            false,
        )
        .unwrap();
        let http_401 = guidance(
            b"error: RPC failed; HTTP 401 curl 22 The requested URL returned error: 401\n",
            b"",
            false,
        );
        assert_eq!(http_401, Some(http));
        if cfg!(target_os = "macos") {
            assert_eq!(ssh, MACOS_SSH_KEY_GUIDANCE);
            assert_eq!(http, MACOS_CREDENTIAL_GUIDANCE);
        } else {
            assert_eq!(ssh, OTHER_SSH_KEY_GUIDANCE);
            assert_eq!(http, OTHER_CREDENTIAL_GUIDANCE);
            for text in [ssh, http] {
                assert!(!text.to_lowercase().contains("keychain"), "{text}");
            }
        }
    }

    #[test]
    fn pull_refusal_uses_terminal_git_diagnostic_after_fetch_progress() {
        let output = b"From /tmp/disposable/upstream\n * branch main -> FETCH_HEAD\nhint: Diverging branches cannot be fast-forwarded.\nfatal: Not possible to fast-forward, aborting.\n";
        for (stderr, stdout) in [
            (output.as_slice(), b"".as_slice()),
            (b"".as_slice(), output.as_slice()),
        ] {
            assert_eq!(
                pull_refusal_headline(stderr, stdout, true),
                Some("Branches have diverged; choose Merge or Rebase to continue.")
            );
        }
    }

    #[test]
    fn unrelated_errors_and_output_names_do_not_claim_pull_divergence() {
        for output in [
            "From /tmp/Not possible to fast-forward, aborting.\nfatal: Authentication failed\n",
            "hint: Not possible to fast-forward, aborting.\nfatal: unable to access remote\n",
            "error: failed to push some refs\n ! [rejected] main -> main (non-fast-forward)\n",
        ] {
            assert!(pull_refusal_headline(output.as_bytes(), b"", true).is_none());
        }
        assert!(
            pull_refusal_headline(
                b"fatal: Not possible to fast-forward, aborting.\n",
                b"",
                false
            )
            .is_none()
        );
    }
}
