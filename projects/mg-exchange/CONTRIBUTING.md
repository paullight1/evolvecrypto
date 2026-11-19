# Contributing to Exchange

Thank you for your interest in contributing.

## Contributor License Agreement (required)

Before any contribution can be merged, you must agree to the
[Contributor License Agreement](CLA.md). The CLA lets the project be offered under
both the open-source AGPL-3.0 license and a commercial license. You retain
copyright to your contribution; you grant the maintainer the rights described in
the CLA.

By opening a pull request you confirm that you have read and agree to the CLA.

## Development setup

```bash
cargo build
cargo test
```

Web client:

```bash
cd clients/web
npm install
npm run dev
```

## Before you open a pull request

- `cargo test` passes with no failures
- `cargo fmt` and `cargo clippy` are clean
- Web client changes: `npx vitest run` passes and
  `npx stylelint 'src/**/*.module.css'` reports 0 violations
- New financial formulas, rates, or thresholds must cite their origin
  (see `docs/references/`)
- Follow the engineering rules in `CLAUDE.md` — never use `f64` for money, keep the
  matching hot path free of async/network, maintain the double-entry ledger invariant

## Pull request guidelines

- Keep changes focused and minimal — one logical change per PR
- Describe what changed and why; link any related issue
- Add or update tests for behavioural changes

## Reporting bugs

Open an issue with reproduction steps. For security vulnerabilities, follow
[SECURITY.md](SECURITY.md) instead — do **not** open a public issue.
