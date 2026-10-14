# Contributing to vers

Thanks for helping make vers better.

## AI-assisted contributions

AI-assisted contributions are welcome. Whether you used an assistant or not, the
same rules apply:

- you understand the change and can explain it in review
- you've run the tests
- the change is focused and the description says what it does and why

## Before you open a merge request

- Discuss larger changes in an issue first.
- Add a fragment. vers uses itself: `vers new <type> <issue> -m "..."`.
- Run `cargo fmt`, `cargo clippy --all-targets` and `cargo test`.
- If behaviour changes, update the docs in `docs/`. They're organised by
  [Diátaxis](https://diataxis.fr/): tutorials, how-to guides, reference and
  concepts. Put new content in the section that matches its purpose.

## Design

See [DESIGN.md](DESIGN.md) for the architecture and the reasoning behind it.
