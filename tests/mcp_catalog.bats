#!/usr/bin/env bats
load test_helper

setup() { setup_test_project; }
teardown() { teardown_test_project; }

catalog_check() {
    python3 "$REPO_ROOT/tests/check_mcp_catalog.py" --bash "$BASH" "$@"
}

@test "pilot catalog has sourced requirements for every selectable variant" {
    run catalog_check --as-of 2026-09-16 --fail-stale
    [ "$status" -eq 0 ]
    [[ "$output" != *"ignored null byte"* ]]
    [[ "$output" == *'"id": "octocode"'* ]]
    [[ "$output" == *'"id": "context7"'* ]]
    [[ "$output" == *'"id": "microsoft-learn"'* ]]
}

@test "requirements sweep reports stale metadata without mutating catalog" {
    cp -R "$REPO_ROOT/catalog/mcp" "$TEST_PROJECT/catalog"
    local before
    before=$(file_sha256 "$TEST_PROJECT/catalog/octocode/manifest.json")
    run catalog_check --catalog "$TEST_PROJECT/catalog" --as-of 2026-12-01 --fail-stale
    [ "$status" -eq 1 ]
    [[ "$output" == *'"status": "due"'* ]]
    [ "$before" = "$(file_sha256 "$TEST_PROJECT/catalog/octocode/manifest.json")" ]
}

@test "requirements metadata rejects credential values" {
    cp -R "$REPO_ROOT/catalog/mcp" "$TEST_PROJECT/catalog"
    python3 -c 'import json,pathlib; p=pathlib.Path("catalog/octocode/manifest.json"); d=json.loads(p.read_text()); d["extensions"]["agentsync.dev"]["variants"]["default"]["auth"]["environment_any_of"]=["TOKEN=fake"]; p.write_text(json.dumps(d))'
    run catalog_check --catalog "$TEST_PROJECT/catalog" --as-of 2026-09-16
    [ "$status" -eq 2 ]
    [[ "$output" == *'names, never values'* ]]
}

@test "evidence URLs reject query and fragment data without printing it" {
    cp -R "$REPO_ROOT/catalog/mcp" "$TEST_PROJECT/catalog"
    local suffix
    for suffix in '?token=private-value' '#token=private-value'; do
        python3 -c 'import json,pathlib,sys; p=pathlib.Path("catalog/octocode/manifest.json"); d=json.loads(p.read_text()); d["extensions"]["agentsync.dev"]["sources"]=["https://docs.example/path"+sys.argv[1]]; p.write_text(json.dumps(d))' "$suffix"
        run catalog_check --catalog "$TEST_PROJECT/catalog" --as-of 2026-09-16
        [ "$status" -eq 2 ]
        [[ "$output" == *'without query or fragment'* ]]
        [[ "$output" != *'private-value'* ]]
    done
}
