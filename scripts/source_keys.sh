#!/usr/bin/env bash
# Enable strict mode only when executed directly, not when sourced.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
fi

target_dir="${TARGET_DIR:-$HOME/.modnet/keys}"
script_path="${KEY_TOOL_SCRIPT:-./scripts/key_tools.py}"

# When executed directly, set a trap to restore terminal state. Avoid altering caller's traps when sourced.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  trap 'stty sane >/dev/null 2>&1 || true' EXIT ERR
fi

if [[ ! -d "$target_dir" ]]; then
  echo "ERROR: target dir does not exist: $target_dir" >&2
  exit 1
fi

# gather files in stable order (only JSON key files and optional nodekey hex files)
mapfile -d '' files < <(find "$target_dir" -maxdepth 1 -type f \( -name '*.json' -o -name 'nodekey-*.hex' \) -print0 | sort -z)

if [[ ${#files[@]} -eq 0 ]]; then
  echo "No key files found in $target_dir" >&2
  exit 0
fi

# clear last values to avoid stale environment
unset PRIVATE_KEY_HEX_AURA || true
unset PUBLIC_KEY_HEX_AURA || true
unset KEY_AURA_PATH || true
unset PRIVATE_KEY_HEX_GRANDPA || true
unset PUBLIC_KEY_HEX_GRANDPA || true
unset KEY_GRANDPA_PATH || true

for i in "${!files[@]}"; do
  idx=$((i + 1))
  file="${files[i]}"
  tmp="$(mktemp)"

  echo "==== Loading key $idx: $file ===="

  base_name_lc="$(basename -- "$file" | tr '[:upper:]' '[:lower:]')"
  if [[ "$base_name_lc" == nodekey-*.hex ]]; then
    # Handle libp2p node key (plain hex). Do not invoke key_tools.py or prompt.
    if [[ -s "$file" ]]; then
      node_hex="$(head -n1 -- "$file" | tr -d '[:space:]')"
      if [[ -n "$node_hex" ]]; then
        export NODE_LIBP2P_KEY="$node_hex"
        export NODE_LIBP2P_KEY_FILE="$file"
        printf 'Loaded NODE_LIBP2P_KEY from %s\n' "$file"
      else
        echo "WARN: nodekey file $file appears empty" >&2
      fi
    else
      echo "WARN: nodekey file $file is empty or unreadable" >&2
    fi
    rm -f "$tmp"
    continue
  fi

  echo "If prompted, type the passphrase in your terminal."

  # run interactively: read input from your terminal, send both stdout+stderr to tee
  # this way you can type when asked, and everything shown on your terminal is also saved to $tmp
  uv run "$script_path" load --file "$file" --with-secret < /dev/tty 2>&1 | tee "$tmp"
  # ensure terminal isn't stuck in raw mode
  stty sane >/dev/null 2>&1 || true

  # extract first {...} JSON block and remove trailing commas (perl handles multi-line)
  json_blob="$(perl -0777 -ne 'if (/(\{.*?\})/s) { $j=$1; $j =~ s/,\s*([}\]])/$1/g; print $j }' "$tmp" || true)"

  if [[ -z "$json_blob" ]]; then
    echo "WARN: no JSON object found in output for $file" >&2
    rm -f "$tmp"
    continue
  fi

  # get private_key_key (prefer jq)
  if command -v jq >/dev/null 2>&1; then
    private_val="$(printf '%s' "$json_blob" | jq -r '.private_key_hex // empty')"
  else
    private_val="$(printf '%s' "$json_blob" | python3 - <<'PY' 2>/dev/null
import sys, json
try:
    obj = json.load(sys.stdin)
except Exception:
    sys.exit(2)
print(obj.get("private_key_hex",""))
PY
    )"
  fi

  if [[ -z "${private_val:-}" ]]; then
    echo "WARN: 'private_key_hex' missing or empty for $file" >&2
    rm -f "$tmp"
    continue
  fi
  public_val="$(printf '%s' "$json_blob" | jq -r '.public_key_hex // empty')"
  if [[ -z "${public_val:-}" ]]; then
    echo "WARN: 'public_key_hex' missing or empty for $file" >&2
    rm -f "$tmp"
    continue
  fi

  # ss58 address (if present)
  ss58_val="$(printf '%s' "$json_blob" | jq -r '.ss58_address // empty')"

  # determine role strictly by filename patterns only
  role=""
  fname_lc="$(basename -- "$file" | tr '[:upper:]' '[:lower:]')"
  if [[ "$fname_lc" == *aura-sr25519* ]]; then
    role="AURA"
  elif [[ "$fname_lc" == *grandpa-ed25519* ]]; then
    role="GRANDPA"
  fi

  # export role-labeled variables or filename-derived variables for other keys
  if [[ -n "$role" ]]; then
    export "PRIVATE_KEY_HEX_${role}=$private_val"
    export "PUBLIC_KEY_HEX_${role}=$public_val"
    [[ -n "$ss58_val" ]] && export "SS58_${role}=$ss58_val"
    export "KEY_${role}_PATH=$file"
  else
    # derive a safe env var prefix from filename
    base_name="$(basename -- "$file")"
    base_name_noext="${base_name%.*}"
    safe_name="$(printf '%s' "$base_name_noext" | tr '[:lower:]' '[:upper:]' | sed -E 's/[^A-Z0-9]+/_/g' | sed -E 's/^_+|_+$//g')"
    if [[ -z "$safe_name" ]]; then
      safe_name="KEY"
    fi
    export "${safe_name}_PRIVATE_HEX=$private_val"
    export "${safe_name}_PUBLIC_HEX=$public_val"
  fi



  # optional visible, comment out if you don't want secrets showing
  if [[ -n "$role" ]]; then
    if [[ -n "$ss58_val" ]]; then
      printf 'Loaded %s key from %s -> exported PRIVATE_KEY_HEX_%s, PUBLIC_KEY_HEX_%s, SS58_%s\n' "$role" "$file" "$role" "$role" "$role"
    else
      printf 'Loaded %s key from %s -> exported PRIVATE_KEY_HEX_%s and PUBLIC_KEY_HEX_%s\n' "$role" "$file" "$role" "$role"
    fi
  else
    printf 'Loaded key from %s -> exported %s_PRIVATE_HEX and %s_PUBLIC_HEX\n' "$file" "$safe_name" "$safe_name"
  fi

  rm -f "$tmp"
done

# Final summary
summary_roles=()
[[ -n "${PRIVATE_KEY_HEX_AURA:-}" ]] && summary_roles+=("AURA")
[[ -n "${PRIVATE_KEY_HEX_GRANDPA:-}" ]] && summary_roles+=("GRANDPA")
if (( ${#summary_roles[@]} > 0 )); then
  printf 'Done. %d file(s) processed. Exported roles: %s.\n' "${#files[@]}" "${summary_roles[*]}"
else
  printf 'Done. %d file(s) processed. No role-based exports were set.\n' "${#files[@]}"
fi

# Final safety: ensure terminal is sane after processing
stty sane >/dev/null 2>&1 || true
