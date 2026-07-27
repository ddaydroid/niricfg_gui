#!/usr/bin/env bash
# Mock niri msg validate runner.
# Accepts a config file path, validates KDL syntax (simulated), exits 0 for valid, 1 for invalid.
# Usage: validate-fake.sh <config-path>

set -euo pipefail

config="${1:?usage: validate-fake.sh <config-path>}"

if [ ! -f "$config" ]; then
    echo "error: config file not found: $config" >&2
    exit 1
fi

# Basic validation: parse KDL structure using a simple heuristic.
# In production, this would call `niri msg validate <config>`.
# For testing, we check:
#   - All braces are balanced
#   - No unclosed strings
#   - No lines with invalid syntax markers

content=$(< "$config")

# Check brace balance
# Note: || true suppresses pipefail when grep finds no matches (empty.kdl).
open_braces=$(grep -o '{' <<< "$content" | wc -l || true)
close_braces=$(grep -o '}' <<< "$content" | wc -l || true)

if [ "$open_braces" -ne "$close_braces" ]; then
    echo "error: unbalanced braces (open=$open_braces close=$close_braces)" >&2
    exit 1
fi

# Check for unclosed strings (odd number of double quotes)
quote_count=$(grep -o '"' <<< "$content" | wc -l || true)
if [ $((quote_count % 2)) -ne 0 ]; then
    echo "error: unclosed string" >&2
    exit 1
fi

# Check for known invalid syntax marker
if grep -q '^// INVALID' <<< "$content"; then
    echo "error: INVALID marker present" >&2
    exit 1
fi

exit 0
