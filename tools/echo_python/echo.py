# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

import sys
import json

try:
    import orjson
except Exception:  # Fallback if orjson is not available; uv should resolve it though
    orjson = None


def to_json_line(obj):
    if orjson is not None:
        return orjson.dumps(obj).decode()
    return json.dumps(obj, separators=(",", ":"))


def main():
    line = sys.stdin.readline()
    if not line:
        print(to_json_line({"isError": True, "error": "no input"}))
        return
    try:
        payload = json.loads(line)
    except Exception as e:
        print(to_json_line({"isError": True, "error": f"invalid JSON: {e}"}))
        return

    args = payload.get("arguments", {})
    text = args.get("text", "")

    # Perform the tool work
    result = {"echo": text}

    # Emit a single JSON line result
    print(to_json_line(result))


if __name__ == "__main__":
    main()
