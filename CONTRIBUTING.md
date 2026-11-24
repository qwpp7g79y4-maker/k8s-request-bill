# Contributing

Thanks for helping. This is a small tool and it stays small.

## Patches

- A patch should come with a test. A bug fix needs a test that fails without
  the fix; a new flag or rule needs a test and a line in `README.md`.
- Patches come in through Issues: open an issue, describe the problem, and
  attach the patch or a link to it. Please do not send anything by other routes.
- Keep a change to one subject. Do not change behaviour and layout in one patch.
- `tests/claims.rs` checks that README and QUALITY match the code. If you add a
  flag, add it to `README.md` as well, and update the test count there.

## Before you send it

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Say in the issue which of these you ran. Anything you did not run, say "not run".

## Security

Do not report vulnerabilities in a public issue. See `SECURITY.md`.

## Licence

By contributing you agree that your work is licensed under MIT OR Apache-2.0,
like the rest of the project.
