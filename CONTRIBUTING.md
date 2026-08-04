# Contributing

This repository contains the buildable source code and packaging for DeckProbe.
Maintainers develop in a private workspace and promote reviewed, quality-gated
snapshots here; every promoted iteration has already passed the maintainers'
correctness and performance gate. Public contributors use normal branches and
pull requests in this repository.

Before opening a pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd packages/deckprobe-js && npm ci && npm run build && npx playwright install chromium && npm test)
```

Treat every input document as untrusted: parsing code must stay within the
bounded read, decompression, and archive-entry budgets.

Never commit document samples or customer content, absolute local paths, build
outputs, passwords, or tokens.
