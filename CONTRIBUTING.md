# Contributing to Backstep

Thank you for helping. Backstep's whole promise is an *exact* replay, so
the bar for correctness is high, and the process below keeps it that way.

## Workflow

1. Open an issue (or comment on one) describing what you want to change.
2. Create a branch from `main`: `m<milestone>-<topic>` for roadmap work
   (for example `m2-decoder`), or `fix-<topic>` / `docs-<topic>`.
3. Make the change **with tests**. Include failure cases.
4. Run `scripts/ci.sh` until it passes.
5. Open a pull request using the template; fill in how it was tested.
6. A pull request is merged when CI is green and the checklist is done.

## Coding standards

**C (recorder, firmware)**
- C99, no dynamic memory, no dependencies beyond `<stdint.h>`/`<stddef.h>`
  in the recorder.
- Compiles cleanly with `-Wall -Wextra -Werror -Wshadow -Wpedantic
  -Wstrict-prototypes -Wmissing-prototypes`.
- Public symbols use the `bs_` prefix; macros use `BACKSTEP_`.

**Rust (host)**
- `cargo fmt`, and `cargo clippy --all-targets -- -D warnings` (pedantic
  lints enabled workspace-wide).
- No `unsafe` code (forbidden at the workspace level).
- Public items are documented; `cargo doc` must build without warnings.

**Everything**
- Every source file starts with a short comment saying what it is for and
  ends its header with `SPDX-License-Identifier: MIT` (C, scripts).
- Design decisions that are hard to reverse get an ADR in `docs/adr/`.

## Commit messages

Use a short imperative summary (≤ 72 characters), a blank line, then what
and why. Reference the milestone or issue, for example `M2: decode
Thumb-2 data-processing instructions`.

## Hardware tests

If a change affects behaviour on real boards, add or update a script in
`scripts/hil/`, a self-test case in `scripts/hil/test_hil_scripts.py`,
and an entry in `docs/HARDWARE_VERIFICATION.md`.

## Releases

1. Add the version's section to `CHANGELOG.md` (`## [x.y.z] - date`) and
   set the version in `host/Cargo.toml` and in `recorder/include/backstep.h`
   (the tests check they agree), in a pull request.
2. After it is merged, tag `main`: `git tag vx.y.z && git push origin vx.y.z`.
3. `.github/workflows/release.yml` builds `backstep` for Linux, macOS (Arm
   and Intel) and Windows, checks each binary reports the tag's version,
   and publishes the release with that changelog section and SHA-256 sums.
   A pull request that changes the workflow or the changelog runs the same
   jobs as a dry run (everything but publishing).

