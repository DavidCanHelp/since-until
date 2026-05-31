# since-until — zsh helper for the `until` binary.
#
# Why this exists: `until` is a zsh *reserved word* (the `until ...; do ...; done`
# loop). The shell recognizes it at parse time, before any PATH lookup, so typing
# a bare `until tomorrow` never reaches the installed binary — it starts a loop and
# drops you into an `until>` continuation prompt (or, with a non-command argument,
# spins forever). This is a shell-naming collision, not a bug in since-until.
#
# We deliberately keep the binary named `until` (to preserve the since/until
# symmetry), and provide this safe wrapper instead.
#
# Install: source this file from your ~/.zshrc, e.g.
#     source /path/to/since-until/contrib/until.zsh
#
# Then use `till` exactly as you'd use `until`:
#     till tomorrow
#     till 2030-01-01
#     till covid
#
# `till` is a natural synonym for "until" and is NOT a reserved word. The wrapper
# uses `command until` internally, which forces an external-command lookup and so
# bypasses the reserved word — it can never recurse back into the keyword.
#
# (You can also always invoke the binary directly with no setup at all:
#     command until tomorrow
#     \until tomorrow
#  Both quote/escape past the reserved word. Aliasing the bare name `until`,
#  however, does NOT work — the reserved word still wins, and an alias body that
#  loops can hang the shell.)

till() {
  command until "$@"
}
