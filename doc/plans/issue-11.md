# Issue #11: Replace default LLM model `gpt-4o-mini` with `gpt-5-nano`

Status: DONE
Issue: https://github.com/tlkahn/ocr-cli/issues/11
Branch: `issue/11-default-model-gpt-5-nano`
Method: strict fine-grained TDD (RED -> GREEN -> refactor) for every
  code-backed work item; docs use "assert/doc drift first" where useful

## 1. Goal

Make `gpt-5-nano` the default LLM model when neither `--model` nor
`LLM_DEFAULT_MODEL` is set. Keep CLI help, README, and test fixtures that
represent "the default model" aligned. Do not change override precedence.

After this work:

| Surface | Expected |
| ------- | -------- |
| `DEFAULT_MODEL` in `src/config.rs` | `"gpt-5-nano"` |
| `Config::resolve` / `from_env` / builder with no model override | `config.model == "gpt-5-nano"` |
| CLI help for `--model` | Documents default `gpt-5-nano` |
| `README.md` options table | Default column `gpt-5-nano` |
| Override order | CLI flag > `LLM_DEFAULT_MODEL` > default (unchanged) |
| Intentional non-default fixtures (e.g. env override to `gpt-4o`) | Unchanged |

### Out of scope

- Provider / base URL changes
- Migrating users who already set `--model` or `LLM_DEFAULT_MODEL`
- Renaming env var `LLM_DEFAULT_MODEL`
- Live API calls to prove `gpt-5-nano` exists (unit/integration stay hermetic)
- Changing title-extraction prompt or behavior beyond the model id string

## 2. Current state (code-verified)

| Item | Status |
| ---- | ------ |
| Source of truth | `src/config.rs:72` `const DEFAULT_MODEL: &str = "gpt-4o-mini";` |
| Resolve path | CLI/override -> `LLM_DEFAULT_MODEL` -> `DEFAULT_MODEL` (`config.rs` ~164-166) |
| Builder path | `self.model.unwrap_or_else(\|\| DEFAULT_MODEL.to_string())` (~356) |
| Config unit tests | Almost all assert via `DEFAULT_MODEL` constant, **not** the literal `"gpt-4o-mini"` |
| Literal pin of default value | **Missing** - changing the constant alone keeps existing asserts green without proving the new id |
| CLI doc comment | `src/cli.rs:28` `[default: gpt-4o-mini]` (clap `Option<String>`, no `default_value`) |
| README | Options table default `gpt-4o-mini` |
| `src/pipeline.rs` tests | Hard-coded `"gpt-4o-mini"` in mock JSON `"model"` fields and `Config { model: ... }` fixtures |
| `src/title.rs` tests | Hard-coded `"gpt-4o-mini"` as `extract_title(..., model, ...)` arg and mock response body |
| Intentional override fixtures | `LLM_DEFAULT_MODEL => "gpt-4o"`, builder `.model("gpt-4o")` - not defaults |
| Visibility | `DEFAULT_MODEL` is private to `config.rs` (tests in same module can see it) |

### Why a new pin test is required

Today:

```rust
assert_eq!(config.model, DEFAULT_MODEL);
```

This only checks "resolved model equals the constant." It does **not** lock the
product default string. Issue #11 is specifically about the product default
becoming `gpt-5-nano`. TDD therefore starts with a failing assertion on the
**literal** value (and/or help/README), then flips the constant.

## 3. Locked decisions

| Topic | Decision | Rationale |
| ----- | -------- | --------- |
| New default id | `gpt-5-nano` exactly | Issue #11 |
| Single source of truth | Keep one `DEFAULT_MODEL` const; production paths read only that | Avoid drift between resolve/builder |
| Pin test style | Assert literal `"gpt-5-nano"` in at least one config test; other tests may keep using `DEFAULT_MODEL` | Locks product value without rewriting every assert |
| CLI default surfacing | Keep `Option<String>` + doc-comment default note; do **not** introduce clap `default_value` unless a failing test forces it | Behavior today: absent flag means "use resolve chain," not "always pass a string through CLI" |
| CLI help verification | Prefer a small test that runs clap help (or debug help string) and asserts `--model` help mentions `gpt-5-nano` | Doc comment alone is easy to leave stale; help is user-visible |
| README | Update table cell; no automated test required in this crate | Docs-only; verify in final grep gate |
| `pipeline` / `title` hard-coded models | Align fixtures that stand in for "a normal config using the default" to `gpt-5-nano` (or a shared test helper string). Mock JSON `"model"` echo fields should match the request model used in that test. Do **not** change intentional alternate models (`gpt-4o`, etc.). | Issue scope; mocks are not a second product default, but stale defaults confuse readers |
| Sharing `DEFAULT_MODEL` across modules | **Do not** `pub` the const just for tests. Either hard-code `"gpt-5-nano"` in foreign-module fixtures or add a narrow `#[cfg(test)]` test helper later only if duplication hurts | Minimize API surface |
| Override regression | Keep existing tests: env overrides default; CLI overrides env; CLI equal-to-default still wins over env | Acceptance criteria |
| Deps | Zero new crates | Project policy |
| Branch | `issue/11-default-model-gpt-5-nano` | Matches issue |
| TDD discipline | Every behavior change: failing test first, minimal code, refactor only while green | User request |

## 4. Design (minimal)

### 4.1 Production change (single line of truth)

```rust
// src/config.rs
const DEFAULT_MODEL: &str = "gpt-5-nano";
```

No resolve/builder logic changes.

### 4.2 CLI comment

```rust
/// LLM model for title extraction [default: gpt-5-nano]
```

### 4.3 README

| Flag | Default | Description |
|------|---------|-------------|
| `--model MODEL` | `gpt-5-nano` | LLM model for title extraction |

### 4.4 Fixture policy

| Location | Treat as | Action |
| -------- | -------- | ------ |
| `config` tests using `DEFAULT_MODEL` | Coupled to const | Leave; greened by const flip |
| New pin test(s) | Product contract | Assert `"gpt-5-nano"` literally |
| `pipeline` `Config { model: "gpt-4o-mini".into(), ... }` | Default-shaped fixture | Change to `"gpt-5-nano"` |
| `pipeline` / `title` mock body `"model": "gpt-4o-mini"` | Echo of requested model | Match the model string that test sends |
| `title` `extract_title(..., "gpt-4o-mini", ...)` | Arbitrary valid model arg | Prefer `"gpt-5-nano"` for consistency with new default (behavior under test is sanitize/error path, not model id) |
| Env/builder overrides to `gpt-4o` | Non-default | Leave |

## 5. TDD cycles

Work one cycle at a time. Do not batch RED tests across cycles.
Commands assume repo root. Prefer narrow test filters while red/green.

---

### Cycle A - Pin resolved default literal (core contract)

**RED**

Add a focused test in `src/config.rs` `mod tests` (near other default tests):

```rust
#[test]
fn test_default_model_is_gpt_5_nano() {
    // Product contract for issue #11: pin the literal, not only DEFAULT_MODEL.
    assert_eq!(DEFAULT_MODEL, "gpt-5-nano");

    let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
    let config = Config::resolve_with(&cli, base_env).unwrap();
    assert_eq!(config.model, "gpt-5-nano");
}
```

Optional same-cycle twin (still RED before const flip) if you want builder covered explicitly:

```rust
#[test]
fn test_builder_default_model_is_gpt_5_nano() {
    let config = Config::builder("sk-mistral", "sk-openai")
        .vault_path("/v")
        .papers_path("/p")
        .build()
        .unwrap();
    assert_eq!(config.model, "gpt-5-nano");
}
```

Prefer **one** pin test first (resolve path); add builder pin only if you want
symmetry. Existing `test_builder_defaults` already uses `DEFAULT_MODEL` and
will follow the const.

Run:

```bash
cargo test --lib test_default_model_is_gpt_5_nano
```

Expect **FAIL**: `DEFAULT_MODEL` is still `"gpt-4o-mini"`.

**GREEN**

```rust
const DEFAULT_MODEL: &str = "gpt-5-nano";
```

Re-run the pin test - pass.

**REFACTOR**

- None expected. If the pin test duplicates `test_defaults_when_no_env_vars`,
  keep both: the old test checks full default bundle via const; the new test
  locks the product string.
- Do **not** weaken the pin to `assert_eq!(config.model, DEFAULT_MODEL)` only.

**Regression pulse**

```bash
cargo test --lib config::
```

All config tests should stay green (they key off the const or explicit
overrides).

---

### Cycle B - CLI help documents the new default

**RED**

Add a test that fails while the doc comment still says `gpt-4o-mini`.

Preferred approach (no new deps): parse help via clap on `Cli`:

```rust
// src/cli.rs mod tests (or config tests if that is where Cli help is already exercised)
#[test]
fn test_model_flag_help_lists_gpt_5_nano_default() {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    let mut help = Vec::new();
    cmd.write_long_help(&mut help).unwrap();
    let help = String::from_utf8(help).unwrap();
    assert!(
        help.contains("gpt-5-nano"),
        "expected --model help to mention default gpt-5-nano, got:\n{help}"
    );
    assert!(
        !help.contains("gpt-4o-mini"),
        "stale default gpt-4o-mini still present in help:\n{help}"
    );
}
```

Notes:

- Clap includes `///` doc comments for long help on args. Confirm with a quick
  local run; if the default only appears in short help, use `write_help`
  instead.
- If help text does **not** include the `[default: ...]` phrase from the doc
  comment wording exactly, assert on the substring that clap actually emits
  after inspecting the RED failure output - still require `gpt-5-nano` and
  forbid `gpt-4o-mini` in help.

Run:

```bash
cargo test --lib test_model_flag_help_lists_gpt_5_nano_default
```

Expect **FAIL** on stale `gpt-4o-mini` or missing `gpt-5-nano`.

**GREEN**

Update `src/cli.rs`:

```rust
/// LLM model for title extraction [default: gpt-5-nano]
```

Re-run - pass.

**REFACTOR**

- If help test is brittle (full help dump), keep assertions limited to the two
  substrings above.
- Do not switch to `default_value = "gpt-5-nano"` unless help test cannot be
  satisfied otherwise - that would change clap's value presence semantics.

**Regression pulse**

```bash
cargo test --lib
```

---

### Cycle C - Fixture alignment (`title` + `pipeline`) as consistency under test

These fixtures do not currently assert "default model id," so a pure behavior
RED is weak. Apply TDD as **characterization then update**:

**RED (characterization you introduce)**

Where a test builds a full `Config` and later asserts on request body model
(if any test inspects the outbound JSON `model` field): add/extend an assert
that the outbound model equals `"gpt-5-nano"` **before** changing fixtures, so
it fails against old hard-coded config.

If **no** test inspects outbound model today (likely for `title` /
`pipeline`):

1. Pick one representative happy-path test in `title.rs` (e.g.
   `test_extract_title_returns_sanitized_title`).
2. RED: change **only the assertion side** first is not available; instead add
   a wiremock request matcher on JSON field `model` == `"gpt-5-nano"` while the
   call still passes `"gpt-4o-mini"` - test fails because request does not match.

Example RED sketch for `title.rs`:

```rust
use wiremock::matchers::{method, path, body_partial_json};

Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .and(body_partial_json(serde_json::json!({ "model": "gpt-5-nano" })))
    .respond_with(/* ... mock body model: "gpt-5-nano" ... */)
    .expect(1)
    .mount(&mock_server)
    .await;

let result = extract_title(
    "some page text...",
    "gpt-5-nano", // GREEN step will set this; for pure RED leave old arg first
    "sk-test",
    &mock_server.uri(),
)
.await;
```

Strict ordering for that test:

1. Add matcher expecting `gpt-5-nano` but leave call arg + mock body on
   `gpt-4o-mini` -> **RED** (mock not hit / mismatch).
2. Update call arg + mock response body to `gpt-5-nano` -> **GREEN**.

Repeat the same pattern for **one** pipeline integration test that performs a
title LLM call, then do a **bulk fixture sweep** (refactor-while-green) for
remaining hard-coded `"gpt-4o-mini"` strings in `pipeline.rs` / `title.rs`
that are default-shaped.

**Do not** touch intentional `"gpt-4o"` override strings in `config.rs` tests.

**GREEN bulk sweep checklist** (after at least one matcher-based cycle):

```bash
rg -n 'gpt-4o-mini' src/ README.md
```

Every remaining hit must be justified (should be **zero** after this issue,
except none expected). Override tests use `gpt-4o`, not `gpt-4o-mini`.

**REFACTOR**

- Optionally introduce a test-only constant in each module:

  ```rust
  #[cfg(test)]
  const TEST_DEFAULT_MODEL: &str = "gpt-5-nano";
  ```

  Only if it removes noisy duplication. Do not export production
  `DEFAULT_MODEL` solely for this.
- Keep mock response `"model"` equal to the requested model in that test.

**Regression pulse**

```bash
cargo test --lib title::
cargo test --lib pipeline::
```

---

### Cycle D - README default column (docs)

Not a rustc test. Treat as checklist with a mechanical gate:

**RED / drift proof**

```bash
rg -n 'gpt-4o-mini' README.md && echo 'STALE' || echo 'CLEAN'
# expect STALE before edit
```

**GREEN**

Update README options row default cell to `gpt-5-nano`.

**VERIFY**

```bash
rg -n 'gpt-5-nano' README.md
rg -n 'gpt-4o-mini' README.md   # expect no matches
```

**REFACTOR**

None.

---

### Cycle E - Full-repo stale string gate + override sanity

**RED intent:** if any stale default remains, fail the gate.

```bash
rg -n 'gpt-4o-mini' .
```

Allow hits only in:

- `doc/plans/issue-11.md` (this plan)
- git history / unrelated local artifacts (should not be in tree)
- Issue text copies if any under `doc/` - prefer updating plan language to past
  tense only after implementation

Production/docs/tests: **zero** `gpt-4o-mini`.

**GREEN:** fix any stragglers found.

**Override sanity (must remain green; do not rewrite to drop coverage):**

```bash
cargo test --lib test_env_model_overrides_default
cargo test --lib test_cli_model_equal_to_default_overrides_env
cargo test --lib test_builder_all_overrides
```

Confirm by reading asserts: env/CLI still win; default pin still
`"gpt-5-nano"` when overrides absent.

## 6. Implementation order (summary)

| Order | Cycle | Primary files | Done when |
| ----- | ----- | ------------- | --------- |
| 1 | A | `src/config.rs` | Pin test green; `DEFAULT_MODEL == "gpt-5-nano"` |
| 2 | B | `src/cli.rs` | Help test green; doc comment updated |
| 3 | C | `src/title.rs`, `src/pipeline.rs` | Matcher + fixture sweep; no stale mini default |
| 4 | D | `README.md` | Table default updated |
| 5 | E | repo-wide | `rg gpt-4o-mini` clean for product paths; full test suite green |

Do not flip `DEFAULT_MODEL` before Cycle A RED exists.
Do not "drive-by" README/cli/fixtures in Cycle A.

## 7. Final verification

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
rg -n 'gpt-4o-mini' src/ README.md   # expect no matches
rg -n 'gpt-5-nano' src/config.rs src/cli.rs README.md
```

Manual smoke (optional, not required for merge if hermetic tests pass):

```bash
# with real keys; should call gpt-5-nano when --model omitted
ocr-cli --dry-run path/to/sample.pdf
```

## 8. PR checklist

- [ ] Cycle A: literal pin test + `DEFAULT_MODEL` flip
- [ ] Cycle B: CLI help test + doc comment
- [ ] Cycle C: title/pipeline fixtures aligned; at least one request matcher on model id
- [ ] Cycle D: README default updated
- [ ] Cycle E: no stale `gpt-4o-mini` under `src/` or `README.md`
- [ ] Override precedence tests still pass
- [ ] `cargo fmt` / `clippy -D warnings` / `cargo test` green
- [ ] PR links https://github.com/tlkahn/ocr-cli/issues/11
- [ ] PR body notes: default only; overrides unchanged; no new deps

## 9. Risk notes

| Risk | Mitigation |
| ---- | ---------- |
| `gpt-5-nano` unknown to API at runtime | Out of scope for unit tests; user-configurable via `--model` / env if needed |
| Help test does not see doc-comment default | Inspect clap help output in RED; adjust assertion to actual emitted text; avoid expanding clap API surface |
| Over-deleting `gpt-4o` override fixtures | Only remove/replace `gpt-4o-mini`; leave explicit `gpt-4o` overrides |
| Publishing `DEFAULT_MODEL` | Disallowed unless a later cycle proves hard need; prefer local test consts |

## 10. References

- Issue: https://github.com/tlkahn/ocr-cli/issues/11
- Const: `src/config.rs` (`DEFAULT_MODEL`)
- Resolve chain: `Config::from_env_with` model block
- CLI flag: `src/cli.rs` `--model`
- Docs: `README.md` Options table
