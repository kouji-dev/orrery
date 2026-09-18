"""A python guest, hand-written against the protocol.

`RpcHost::python` runs `python main.py` — the same RPC path as node, a different
argv. There is no python SDK and this deliberately does not pretend to be one:
it is here to prove the transport is language-shaped, not node-shaped.
"""

import json
import os
import re
import sys

CONTENT_LENGTH = re.compile(rb"^content-length:\s*(\d+)\s*$", re.IGNORECASE)


def send(message):
    body = json.dumps(message).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.rstrip(b"\r\n")
        if line == b"":
            break
        match = CONTENT_LENGTH.match(line)
        if match:
            length = int(match.group(1))
    if length is None:
        sys.exit(2)
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))


def main():
    while True:
        message = read_message()
        if message is None:
            return
        method = message.get("method")
        if method is None:
            continue
        ident = message.get("id")
        params = message.get("params") or {}

        if method == "ext/load":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": ident,
                    "result": {"tools": [{"name": "version", "description": "Which python."}]},
                }
            )
        elif method == "tool/call":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": ident,
                    "result": {
                        "outcome": {
                            "t": "ok",
                            "value": {
                                "major": sys.version_info[0],
                                "cwd_is_extension_root": os.path.isfile("main.py"),
                                "tool": params.get("tool"),
                            },
                        }
                    },
                }
            )
        elif method == "ext/shutdown":
            send({"jsonrpc": "2.0", "id": ident, "result": None})
            return
        elif ident is not None:
            send(
                {
                    "jsonrpc": "2.0",
                    "id": ident,
                    "error": {"code": -32601, "message": "no such method: %s" % method},
                }
            )


if __name__ == "__main__":
    main()
