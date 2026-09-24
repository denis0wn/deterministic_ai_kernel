#!/usr/bin/env python3
import argparse
import json
import os
import socket
import socketserver
import sys
import tempfile
from functools import lru_cache


def _read_json_line(stream):
    line = stream.readline()
    if not line:
        raise EOFError("empty input")
    return json.loads(line)


def _write_json_line(stream, payload):
    stream.write(json.dumps(payload) + "\n")
    stream.flush()


@lru_cache(maxsize=1)
def load_generate_function():
    try:
        from mlx_lm import load, generate
    except Exception as exc:
        raise RuntimeError(f"failed to import mlx_lm: {exc}") from exc
    return load, generate


@lru_cache(maxsize=1)
def load_model_and_tokenizer(model_path: str):
    load, _ = load_generate_function()
    return load(model_path)


class BridgeRequestHandler(socketserver.StreamRequestHandler):
    def handle(self):
        try:
            request = _read_json_line(self.rfile)
            prompt = request.get("prompt", "")
            if not isinstance(prompt, str) or not prompt.strip():
                raise RuntimeError("prompt is empty")

            _, generate = load_generate_function()
            model, tokenizer = load_model_and_tokenizer(self.server.model_path)
            output_text = generate(model, tokenizer, prompt=prompt, verbose=False)

            _write_json_line(
                self.wfile,
                {
                    "request_id": request.get("request_id", "unknown"),
                    "output_text": output_text.strip(),
                    "finished": True,
                },
            )
        except Exception as exc:
            _write_json_line(
                self.wfile,
                {
                    "error": str(exc),
                    "finished": True,
                },
            )


class BridgeServer(socketserver.ThreadingUnixStreamServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(self, socket_path: str, model_path: str):
        if os.path.exists(socket_path):
            os.remove(socket_path)
        self.model_path = model_path
        super().__init__(socket_path, BridgeRequestHandler)


def run_server(model_path: str, socket_path: str) -> int:
    server = BridgeServer(socket_path=socket_path, model_path=model_path)
    try:
        load_model_and_tokenizer(model_path)
        print(json.dumps({"status": "ready", "socket_path": socket_path}), flush=True)
        server.serve_forever()
        return 0
    finally:
        server.server_close()
        if os.path.exists(socket_path):
            os.remove(socket_path)


def run_once(model_path: str) -> int:
    try:
        request = _read_json_line(sys.stdin)
    except Exception as exc:
        print(json.dumps({"error": f"invalid request json: {exc}"}), file=sys.stderr)
        return 1

    try:
        _, generate = load_generate_function()
        model, tokenizer = load_model_and_tokenizer(model_path)
        prompt = request.get("prompt", "")
        if not isinstance(prompt, str) or not prompt.strip():
            raise RuntimeError("prompt is empty")

        output_text = generate(model, tokenizer, prompt=prompt, verbose=False)
        print(
            json.dumps(
                {
                    "request_id": request.get("request_id", "unknown"),
                    "output_text": output_text.strip(),
                    "finished": True,
                }
            )
        )
        return 0
    except Exception as exc:
        print(json.dumps({"error": str(exc)}), file=sys.stderr)
        return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--daemon", action="store_true")
    parser.add_argument("--socket-path", default=os.path.join(tempfile.gettempdir(), "deterministic_ai_kernel_mlx.sock"))
    args = parser.parse_args()

    if args.daemon:
        return run_server(model_path=args.model, socket_path=args.socket_path)
    return run_once(model_path=args.model)


if __name__ == "__main__":
    raise SystemExit(main())
