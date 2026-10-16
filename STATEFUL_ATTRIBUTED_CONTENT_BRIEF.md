# S1 bounded attributed-content implementation brief

Add a private scalar loader beside effective_hit, leaving native reads unchanged.
It requires caller-owned admission and transaction, uses the existing ModelTool
eligibility policy, distinguishes incomplete coverage, and checks requested
structure. SQLite CASE guards bound text before Rust allocation: IDs/source512,
enums32, content2048 UTF-8 bytes. No support/context/source payload is selected.
Clock and current-pointer admission remain mandatory before this loader is exposed.

Stage one freezes the private loader and contract tests with fresh Droid review.
Stage two attaches the reviewed loader to the compiled-domain reader in one fresh
admitted snapshot, returning explicit historical projection/omissions and no
verification claim. CLI, migration, native routing and MCP remain later slices.
Tests compare native scalar parity, refusal boundaries, retirement/coverage and
fresh snapshots; full metadata admission precedence remains unchanged.
