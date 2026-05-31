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
