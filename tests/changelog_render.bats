#!/usr/bin/env bats
# How `agentsync update` prints a CHANGELOG section. The changelog is Markdown
# written for GitHub, so a terminal must not show its inline markers literally
# or run a paragraph past the window edge.

load test_helper

setup() {
    setup_test_project
    # shellcheck disable=SC1090,SC1091
    source "$REPO_ROOT/lib/helpers/cli_colors.sh"
    # shellcheck disable=SC1090,SC1091
    source "$REPO_ROOT/lib/helpers/update.sh"

    FIXTURE="$TEST_PROJECT/CHANGELOG.md"
    cat > "$FIXTURE" <<'EOF'
# Changelog

## 9.9.9

A summary line that is deliberately long enough to need wrapping when it is printed into a terminal of ordinary width, several times over, so the wrap is unmistakable.

### Internal

- **A bold lead-in.** Body text mentioning `code_span` and a second `span`, padded out so this bullet is certainly longer than any sensible terminal width and must wrap onto continuation lines.
- A short bullet.

## 9.9.8

- Older entry that must not appear.
EOF
}

teardown() {
    teardown_test_project
}

# ── inline markers ──────────────────────────────────────────────────────────

@test "changelog: bold markers are stripped, not printed literally" {
    _md_plain '**Bold.** rest'
    [ "$REPLY" = "Bold. rest" ]
}

@test "changelog: backticks are stripped" {
    _md_plain 'run `agentsync sync` now'
    [ "$REPLY" = "run agentsync sync now" ]
}

@test "changelog: text without markers is untouched" {
    _md_plain 'plain text — with an em dash'
    [ "$REPLY" = "plain text — with an em dash" ]
}

@test "changelog: rendered output carries no markdown markers" {
    run _show_changelog_sections "$FIXTURE" "9.9.9"
    [ "$status" -eq 0 ]
    [[ "$output" != *'**'* ]]
    [[ "$output" != *'`'* ]]
}

# ── wrapping ────────────────────────────────────────────────────────────────

@test "changelog: no rendered line exceeds the chosen width" {
    local width=72
    run _print_wrapped "$(printf 'word %.0s' $(seq 1 80))" "    • " "      " "$width"
    [ "$status" -eq 0 ]
    local line
    while IFS= read -r line; do
        [ "${#line}" -le "$width" ]
    done <<< "$output"
}

@test "changelog: continuation lines are indented under the first" {
    run _print_wrapped "$(printf 'word %.0s' $(seq 1 40))" "    * " "      " 60
    [ "$status" -eq 0 ]
    local first second
    first=$(printf '%s\n' "$output" | sed -n '1p')
    second=$(printf '%s\n' "$output" | sed -n '2p')
    [[ "$first" == "    * word"* ]]
    [[ "$second" == "      word"* ]]
}

@test "changelog: short text stays on one line" {
    run _print_wrapped "brief" "    • " "      " 80
    [ "$output" = "    • brief" ]
}

@test "changelog: a pathological width still yields a usable line" {
    run _print_wrapped "alpha beta gamma delta" "  " "  " 1
    [ "$status" -eq 0 ]
    [ -n "$output" ]
    [[ "$output" == *alpha* ]]
}

# ── width probing ───────────────────────────────────────────────────────────

@test "changelog: width falls back to 80 when tput reports nothing usable" {
    PATH="$TEST_PROJECT/nobin" _changelog_width
    [ "$REPLY" -eq 80 ]
}

@test "changelog: width is clamped into a readable range" {
    _changelog_width
    [ "$REPLY" -ge 40 ]
    [ "$REPLY" -le 100 ]
}

# ── section selection ───────────────────────────────────────────────────────

@test "changelog: only the requested version is printed" {
    run _show_changelog_sections "$FIXTURE" "9.9.9"
    [[ "$output" == *"What's new in v9.9.9"* ]]
    [[ "$output" == *"A short bullet"* ]]
    [[ "$output" != *"must not appear"* ]]
}

@test "changelog: the section heading survives rendering" {
    run _show_changelog_sections "$FIXTURE" "9.9.9"
    [[ "$output" == *"Internal"* ]]
}

@test "changelog: every rendered line fits the clamped width" {
    local width
    _changelog_width
    width="$REPLY"

    run _show_changelog_sections "$FIXTURE" "9.9.9"
    [ "$status" -eq 0 ]
    local line
    while IFS= read -r line; do
        [ "${#line}" -le "$width" ]
    done <<< "$output"
}
