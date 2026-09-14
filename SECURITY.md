# Security

Please do not report vulnerabilities in public issues, pull requests, or
discussions, and do not attach credentials or a private repository.

The planned private reporting route is
[GitHub private vulnerability reporting](https://github.com/FernandoX7/GitTurtle/security/advisories/new).
**It is not active while this repository remains private.** At publication, the
maintainer must enable it and verify that **Report a vulnerability** appears on
the repository's Security → Advisories page before accepting public reports.
No security email address is provided. The
[launch checklist](docs/public-launch.md) tracks activation.

Once that private route is active, include the affected version or commit, OS,
impact, reproduction steps, and a minimal disposable repository or sanitized
sample. Explain whether opening, previewing, or explicitly running a Git action
triggers the issue. Share only the minimum information needed to reproduce it.

Security fixes currently target the latest source on `main`; no release support
window or response-time commitment is established. Repository contents, Git
helpers/hooks, file decoders, authentication, and explicit GitHub/network
operations are all relevant security boundaries.
