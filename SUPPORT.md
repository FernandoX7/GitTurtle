# Getting help

Start with the [user guide](docs/user-guide.md), [Linux setup and troubleshooting](docs/linux.md), and [preview format matrix](docs/file-previews.md). GitTurtle is in early development; the [platform validation record](docs/validation.md) explains what has been exercised.

- **Something is broken:** use the [bug report form](https://github.com/FernandoX7/GitTurtle/issues/new?template=bug_report.yml), including steps to reproduce and the app build and OS.
- **You have an idea:** use the [feature request form](https://github.com/FernandoX7/GitTurtle/issues/new?template=feature_request.yml) to describe the task you want to make easier.
- **You need help using GitTurtle:** search [existing issues](https://github.com/FernandoX7/GitTurtle/issues) first, then [open a support question](https://github.com/FernandoX7/GitTurtle/issues/new?labels=question&title=Help%3A%20).
- **You found a possible security issue:** follow [SECURITY.md](SECURITY.md) for private reporting.

Community help is offered as time allows. There is no response-time guarantee.

## Share useful, safe diagnostics

Include the app version and, for a source build, its commit ID and whether the checkout has local changes. The Linux bundle's `build-info.json` identifies the packaging revision and executable hash; a reused binary may have a different source identity. Describe your OS version, CPU architecture, and relevant display or graphics details. On Linux, include the desktop environment and Wayland or X11 session type, plus the scale setting for display issues.

A minimal example in a disposable repository is usually more useful than a full log. Before attaching logs or screenshots, remove access tokens, passwords, private keys, authenticated remote URLs, personal names and email addresses, private paths, commit messages and source content. Share only the excerpt needed to explain the failure. Do not attach your entire environment, Git configuration or application data directory: those can contain private information and saved drafts.

## Support development

You can [sponsor GitTurtle's development on GitHub](https://github.com/sponsors/FernandoX7) with a one-time or monthly contribution. Sponsorship supports maintenance, bug fixes, documentation and platform testing; it does not purchase a feature or a response-time commitment.

Testing on your platform, improving documentation, submitting focused fixes and helping another user all support the project. See [CONTRIBUTING.md](CONTRIBUTING.md) to get started.
