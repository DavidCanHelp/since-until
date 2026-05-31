# since-until

Honest, calendar-aware date differences with user-defined named anchors.
**One engine, three front doors.**

```
cargo install since-until
```

…installs three binaries that all share one engine and one anchors file:

| binary             | what it is                                                        |
| ------------------ | ----------------------------------------------------------------- |
| `since`            | CLI, past-leaning — "6 years, 2 months, 30 days ago"              |
| `until`            | CLI, future-leaning — "in 6 months, 24 days" (gentle note if past)|
| `since-until-mcp`  | MCP server (stdio) — `since`, `until`, `list_anchors` tools       |

## zsh users: read this about `until`

`until` is a **zsh reserved word** (the `until …; do …; done` loop). zsh recognizes
it at parse time, *before* any PATH lookup, so a bare `until tomorrow` never reaches
the installed binary — instead you drop into an `until>` continuation prompt (and with
a non-command argument it can loop forever). `since` is unaffected. This is a shell
naming collision, **not** a bug — `since-until` installs the binary correctly at
`~/.cargo/bin/until`; the shell just intercepts the name first.

Two ways to run it with **zero setup** — both quote/escape past the reserved word:

```sh
command until tomorrow
\until tomorrow
```

For everyday use, add a short, safe wrapper to your `~/.zshrc` (`till` is a natural
synonym and is *not* reserved):

```sh
# since-until: `until` is a zsh reserved word; `till` calls the real binary.
till() { command until "$@"; }
```

Then `till tomorrow`, `till 2030-01-01`, `till covid` all work. The wrapper uses
`command until` internally, so it can never recurse into the keyword. A ready-to-source
version lives at [`contrib/until.zsh`](contrib/until.zsh):

```sh
source /path/to/since-until/contrib/until.zsh
```

> Note: aliasing the bare name (`alias until=…`) does **not** work — the reserved word
> still wins, and a looping alias body can hang the shell. Use `command until`, `\until`,
> or the `till` function. **bash** users are unaffected; `until` is reserved there too,
> but a bare `until tomorrow` simply errors rather than shadowing — still, the same
> wrappers work if you want them.
>
> Why not just rename the binary? Keeping `until` preserves the `since` / `until`
> symmetry that's the whole point of the tool, and the binary itself is correct on every
> platform and shell that doesn't reserve the word. The collision is purely lexical, so
> the fix belongs in your shell config, not in the command's name.

## The engine

A signed, calendar-honest difference between a target date and *now*, broken into
years / months / days plus a direction (`past` / `future` / `today`) and a humanized
sentence. Month-ends and leap years are handled the dateutil way — step back whole
months without overshooting, then measure the leftover days — so `Jan 31 → Mar 1`
is "1 month, 1 day", not a day-borrow underflow.

## Tokens & anchors

Every command takes a **token**: either an ISO date (`YYYY-MM-DD`) or an *anchor
nickname*. Resolution order is ISO-parse first, then nickname lookup, then a clear
error — so a literal date can never be shadowed by an anchor.

```sh
since 2020-03-01            # 6 years, 2 months, 30 days ago
since anchor add covid 2020-03-01
since covid                 # covid (2020-03-01): 6 years, 2 months, 30 days ago
until covid                 # ... ago  (heads up — that date has already passed)
since anchor list
since anchor remove covid
```

Anchors are a flat `nickname -> ISO date` JSON map at the platform config dir
(e.g. `~/.config/since-until/anchors.json`), hand-editable, case-insensitive,
shared by all three binaries. A missing file is simply an empty set.

## MCP

`since-until-mcp` speaks MCP over stdio (built on [`rmcp`](https://crates.io/crates/rmcp)).
Tools return **structured** output — the numbers *and* the sentence:

```json
{ "token": "covid", "date": "2020-03-01", "years": 6, "months": 2, "days": 30,
  "direction": "past", "humanized": "6 years, 2 months, 30 days ago" }
```

`list_anchors` returns the same nickname map the CLIs manage, so anchors added on
the command line are immediately visible to the model.

## License

MIT
