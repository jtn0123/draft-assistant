# Security policy

## Supported versions

Only the current `main` branch is supported. There are no maintained release
branches.

## Reporting a vulnerability

Report privately through GitHub's
[private vulnerability reporting](https://github.com/jtn0123/draft-assistant/security/advisories/new)
rather than opening a public issue. Expect a first response within a week; this
is a personal project, not a staffed one.

## Scope, honestly

This is a local-first desktop app. It reads the public Sleeper API and, with
the user's own credentials, the Yahoo Fantasy API; it writes nothing back to
either. Secrets (the Anthropic API key, Yahoo OAuth tokens) live in the macOS
keychain, never in the repo or in the app's data directory.

The macOS bundle is **ad-hoc signed and not notarized** — there is no Apple
developer account behind it. That is a known, documented limitation, not a
vulnerability report; see "Installing on macOS" in the README.
