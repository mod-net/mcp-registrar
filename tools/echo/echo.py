#!/usr/bin/env python3
import sys
import json

def main():
    line = sys.stdin.readline()
    if not line:
        print(json.dumps({"content": [{"type": "text", "text": ""}], "isError": True}))
        return
    try:
        payload = json.loads(line)
    except Exception as e:
        print(json.dumps({"content": [{"type": "text", "text": f"invalid JSON: {e}"}], "isError": True}))
        return

    args = payload.get("arguments", {})
    text = args.get("text", "")

    # Emit MCP-native content array with text
    out = {"content": [{"type": "text", "text": text}], "isError": False}
    print(json.dumps(out, separators=(",", ":")))

if __name__ == "__main__":
    main()
