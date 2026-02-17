# Security Policy

## Supported Versions

Only the latest released version of mergers receives security fixes. Older versions are not patched.

| Version | Supported |
| ------- | --------- |
| 0.3.x   | Yes       |
| < 0.3   | No        |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Use one of the following channels:

- **GitHub Security Advisories (preferred):** Open a private advisory at
  `https://github.com/egeapak/mergers/security/advisories/new`
- **Email:** Send details to the repository maintainer via the email listed on
  their GitHub profile.

When reporting, please include:

- A clear description of the vulnerability and its potential impact
- Steps to reproduce the issue
- Any relevant configuration, environment details, or log output
- If known, a suggested fix or mitigation

## What Constitutes a Security Issue

mergers is a CLI/TUI tool that interacts with Azure DevOps APIs, performs git
operations, and reads credentials from environment variables or configuration
files. The following are considered security issues:

- **Credential exposure** — PAT tokens, passwords, or secrets written to
  unintended locations (logs, state files, terminal output)
- **Arbitrary command execution** — user-controlled input passed unsanitised to
  shell commands or git operations
- **Path traversal** — file operations that escape intended directories (e.g.,
  worktree or state-file paths)
- **Privilege escalation** — operations that acquire more system access than
  intended
- **Insecure state files** — sensitive data written to world-readable files or
  without appropriate permissions
- **Dependency vulnerabilities** — known CVEs in transitive dependencies that
  affect mergers' attack surface

The following are **not** considered security issues:

- Bugs that require the attacker to already have full control of the machine
- Issues in development-only dependencies (`[dev-dependencies]`) that do not
  affect the compiled binary
- Cosmetic or usability bugs in TUI rendering

## Disclosure Policy

Once a fix is released, vulnerabilities will be disclosed publicly via a GitHub
Security Advisory. Credit will be given to the reporter unless they prefer to
remain anonymous.
