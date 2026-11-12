#!/usr/bin/env bash
# E2E preflight for the Hakobi GitHub pipeline.
# Usage: scripts/gh-e2e-preflight.sh [base_url] [username] [password]
# Prints one line per check: PASS/FAIL/WAIT + what it proves.
set -euo pipefail

BASE_URL="${1:-https://console.glicc.id}"
USER="${2:-admin}"
PASS="${3:-HakobiConsole2026!}"
JAR="$(mktemp)"
trap 'rm -f "$JAR"' EXIT

step() { printf '%-6s %s\n' "$1" "$2"; }

# 1. Up
code=$(curl -s -o /dev/null -w '%{http_code}' "$BASE_URL/healthz")
[ "$code" = "200" ] && step PASS "healthz 200" || { step FAIL "healthz=$code"; exit 1; }

# 2. Login (CSRF dance)
csrf=$(curl -s -c "$JAR" "$BASE_URL/login" | grep -o 'name="csrf_token" value="[^"]*"' | sed 's/.*value="//;s/"//')
code=$(curl -s -b "$JAR" -c "$JAR" -o /dev/null -w '%{http_code}' -X POST \
  -H "Origin: $BASE_URL" \
  --data-urlencode "username=$USER" \
  --data-urlencode "password=$PASS" \
  --data-urlencode "csrf_token=$csrf" \
  "$BASE_URL/login")
[ "$code" = "303" ] && step PASS "login 303" || { step FAIL "login=$code"; exit 1; }

# 3. GitHub status
status=$(curl -s -b "$JAR" "$BASE_URL/api/github/status")
installed=$(echo "$status" | grep -o '"app_installed":[a-z]*' | cut -d: -f2)
registered=$(curl -s -b "$JAR" "$BASE_URL/settings" -o /dev/null -w '%{http_code}')
[ "$registered" = "200" ] && step PASS "settings page renders" || step FAIL "settings=$registered"

# 4. State machine
if [ "$installed" = "true" ]; then
  step PASS "app installed"
  step NEXT  "run pipeline test: push to a repo matched by an environment"
else
  # 5. Ask GitHub directly via our recheck endpoint (server-side app JWT)
  code=$(curl -s -b "$JAR" -o /tmp/recheck.html -w '%{http_code}' -X POST \
    -H "Origin: $BASE_URL" "$BASE_URL/settings/github/recheck")
  if [ "$code" = "400" ]; then
    step PASS "clean state — no app registered yet"
    step NEXT  "Settings -> Install GitHub App -> Create GitHub App -> Install -> Check installation"
  elif grep -q "No installation found" /tmp/recheck.html 2>/dev/null; then
    step WAIT "registered but NOT installed on github.com"
    step NEXT  "Settings -> Install on GitHub -> select repos -> Install -> Check installation"
  elif [ "$code" = "303" ]; then
    step PASS "installation found and recorded"
  else
    step FAIL "recheck unexpected code=$code"
  fi
fi

# 6. Webhook endpoint reachable and HMAC-enforced (unsigned must 401/501)
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$BASE_URL/webhooks/github" -d '{}')
case "$code" in
  401|501|403) step PASS "webhook rejects unsigned ($code)" ;;
  *) step FAIL "webhook accepted unsigned body ($code)" ;;
esac

rm -f /tmp/recheck.html 2>/dev/null || true
