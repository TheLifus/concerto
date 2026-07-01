# Contributing

Run before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo deny check
```

Code standards:

- Keep lines at 100 characters or less.
- Keep functions under 50 lines when practical; 80 lines is the hard Clippy limit.
- Keep files under 300 lines when practical; split before 500 lines.
- Keep function arguments to 5 or fewer; use a struct when more are needed.
- Do not use `unwrap()` or `expect()` outside tests.
- Add one focused test for non-trivial parsing, branching, filesystem, network, or security logic.
- Add dependencies only when the standard library or an existing dependency is not enough.
