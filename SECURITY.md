# Security Policy

DeckProbe is designed with a security-first mindset: **all input documents are treated as untrusted**. The engine inspects PDF, Microsoft Office, and Apple iWork files without rendering them or launching desktop office suites, minimizing attack surface while extracting targeted metadata.

## Supported Versions

We actively maintain and provide security updates for the following versions:

| Version | Status          |
| ------- | --------------- |
| Latest stable minor (X.Y) | ✅ Supported    |
| Latest beta (X.Y-beta.Z)  | ✅ Supported    |
| Older minor versions      | ❌ Unsupported  |

We strongly recommend running the latest published stable or beta release to benefit from the most recent security hardening.

## Reporting a Vulnerability

We appreciate responsible disclosure of security issues. If you discover a vulnerability, please follow these steps:

### 1. **Do Not** Create Public Issues
Avoid opening public GitHub issues, discussions, or posting details on social media that could expose users to risk before a fix is available. This includes:
- Malicious sample documents
- Exploit code or proof-of-concept scripts
- Sensitive data (passwords, customer information, internal paths)

### 2. Submit a Private Report
Report suspected vulnerabilities **privately** via one of these channels:

- **GitHub Security Advisories**: Use the ["Report a vulnerability"](https://github.com/deckflow/deckprobe/security/advisories/new) feature in the `deckflow/deckprobe` repository.
- **Email**: Send details to [security@deckprobe.dev](mailto:security@deckprobe.dev) (if you prefer email).

For sensitive reports, you may encrypt your message using our PGP key (available upon request or via GitHub Security Advisories).

### 3. What to Include
To help us triage efficiently, please provide:
- A clear description of the vulnerability and its potential impact
- Steps to reproduce (including sample files, if safe to share privately)
- Affected DeckProbe version(s) and platform (CLI, SDK, browser)
- Any known workarounds or mitigations

## Our Commitment to You

- **Acknowledgment**: We will confirm receipt of your report within **48 hours**.
- **Assessment**: We aim to evaluate severity and provide an initial response within **5 business days**.
- **Resolution Timeline**: For confirmed vulnerabilities, we target a fix within **30 days**, depending on complexity.
- **Disclosure Coordination**: We will coordinate with you on public disclosure timing. Typically, we publish advisories after patches are released and users have had time to upgrade.
- **Credit**: With your permission, we will acknowledge your contribution in our security advisories and release notes.

## Scope

This policy covers:
- All official DeckProbe crates (`deckprobe-*`) published under the `deckflow` organization
- The DeckProbe CLI tool
- The Browser SDK (`@deckprobe/js`)
- Official Docker images and distribution packages

Out of scope:
- Third-party applications integrating DeckProbe
- Vulnerabilities requiring physical access or already-disclosed zero-days in underlying parsers (e.g., PDF, OOXML libraries) unless exacerbated by DeckProbe's handling

## Security Best Practices for Users

- Always run the latest supported version
- Validate document sources before processing
- Use DeckProbe in sandboxed or isolated environments when handling highly sensitive or unknown files
- Restrict file system permissions for the DeckProbe process

---

Thank you for helping keep DeckProbe and its users safe. Your responsible disclosure makes the ecosystem more secure for everyone.
