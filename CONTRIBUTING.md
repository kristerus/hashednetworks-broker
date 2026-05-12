# Contributing to HashedNetworks Broker

Thanks for considering a contribution. This document covers the basics.

## Reporting bugs

File an issue at <https://github.com/kristerus/hashednetworks-broker/issues>.
Include:

* OS + version (`uname -a` / Windows build).
* `broker --version` (or the docker image tag / commit SHA).
* Exact reproduction steps — config, commands, env vars.
* Observed vs expected behaviour, plus any log excerpts (`docker compose
  logs broker`).

## Pull requests

1. Fork the repo and branch from `main`.
2. Run the local checks before pushing:

   ```sh
   cargo fmt
   cargo clippy --all-targets -- -D warnings
   cargo test --all
   ```

3. Push to your fork and open a PR.
4. CI must pass (formatting, clippy, tests, build). PRs are blocked
   otherwise.
5. **Sign off** each commit using the
   [Developer Certificate of Origin](https://developercertificate.org/):

   ```sh
   git commit -s -m "your message"
   ```

   The sign-off is a one-line statement that you have the right to submit
   the patch under the project's licence.

## Code style

* Rust 2021 edition, formatted with `cargo fmt` (committed `rustfmt.toml`
  is authoritative).
* `cargo clippy --all-targets -- -D warnings` must succeed.
* Tests live alongside the code they test (`#[cfg(test)] mod tests`) and
  in `tests/` for integration tests.
* No `unwrap()` / `expect()` in non-test code paths — return `Result` and
  propagate. Tests are exempt for readability.

## Security issues

**Please do not file public issues for vulnerabilities.** See
[SECURITY.md](./SECURITY.md) for the coordinated-disclosure process.

## Licensing

By contributing you agree your work will be licensed under the Apache
2.0 licence in [LICENSE](./LICENSE).
