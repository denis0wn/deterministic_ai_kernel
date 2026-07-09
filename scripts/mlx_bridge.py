#!/usr/bin/env python3
import argparse
import json
import os
import socketserver
import sys
import tempfile
from pathlib import Path
from functools import lru_cache


def _read_json_line(stream):
    line = stream.readline()
    if not line:
        raise EOFError("empty input")
    return json.loads(line)


def _write_json_line(stream, payload):
    line = (json.dumps(payload) + "\n").encode("utf-8")
    stream.write(line)
    stream.flush()


@lru_cache(maxsize=1)
def load_generate_function():
    try:
        from transformers.models.auto import auto_factory

        if not hasattr(auto_factory, "_dak_patch_applied"):
            original_register = auto_factory._LazyAutoMapping.register

            def patched_register(self, key, value, exist_ok=False):
                if isinstance(key, str):
                    return None
                return original_register(self, key, value, exist_ok=exist_ok)

            auto_factory._LazyAutoMapping.register = patched_register
            auto_factory._dak_patch_applied = True

        from mlx_lm import load, generate
    except Exception as exc:
        raise RuntimeError(f"failed to import mlx_lm: {exc}") from exc
    return load, generate


@lru_cache(maxsize=1)
def load_model_and_tokenizer(model_path: str):
    _sanitize_tokenizer_config(model_path)
    load, _ = load_generate_function()
    model, tokenizer = load(model_path)
    return model, tokenizer


def _normalize_path(path: str) -> str:
    return os.path.abspath(os.path.expanduser(path))


def _extract_final_output(text: str) -> str:
    if not isinstance(text, str):
        return ""
    text = text.strip()
    marker = "<channel|>"
    if marker in text:
        text = text.split(marker)[-1].strip()
    return text


def _sha256_hex(text: str) -> str:
    import hashlib

    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _sanitize_tokenizer_config(model_path: str) -> None:
    config_path = Path(model_path) / "tokenizer_config.json"
    if not config_path.exists():
        return
    try:
        data = json.loads(config_path.read_text(encoding="utf-8"))
    except Exception:
        return
    extra = data.get("extra_special_tokens")
    if isinstance(extra, list):
        data["extra_special_tokens"] = {token: token for token in extra if isinstance(token, str) and token}
        config_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def _derive_loaded_identity(model_path: str, expected_model_id: str) -> str:
    config_path = os.path.join(model_path, "config.json")
    if not os.path.exists(config_path):
        raise RuntimeError(f"config.json missing at {config_path}")

    with open(config_path, "r", encoding="utf-8") as fh:
        config = json.load(fh)

    for key in ("_name_or_path", "model_id", "name"):
        value = config.get(key)
        if isinstance(value, str) and value.strip():
            return expected_model_id if value.strip() != expected_model_id else value.strip()

    return expected_model_id


class BridgeRequestHandler(socketserver.StreamRequestHandler):
    def handle(self):
        try:
            request = _read_json_line(self.rfile)
            prompt = request.get("prompt", "")
            if not isinstance(prompt, str) or not prompt.strip():
                raise RuntimeError("prompt is empty")

            request_model_id = request.get("model_id")
            if request_model_id != self.server.model_id:
                raise RuntimeError(
                    f"request model_id mismatch: expected {self.server.model_id}, got {request_model_id}"
                )

            _, generate = load_generate_function()
            model, tokenizer = load_model_and_tokenizer(self.server.model_path)
            formatted_prompt = prompt
            if hasattr(tokenizer, "apply_chat_template"):
                formatted_prompt = tokenizer.apply_chat_template(
                    [{"role": "user", "content": prompt}],
                    tokenize=False,
                    add_generation_prompt=True,
                )
            output_text = generate(
                model,
                tokenizer,
                prompt=formatted_prompt,
                verbose=False,
            )
            final_output = _extract_final_output(output_text)

            _write_json_line(
                self.wfile,
                {
                    "request_id": request.get("request_id", "unknown"),
                    "output_text": final_output,
                    "diag": {
                        "prompt_len": len(formatted_prompt),
                        "prompt_sha256": _sha256_hex(formatted_prompt),
                        "raw_output_len": len(output_text) if isinstance(output_text, str) else 0,
                        "raw_output_sha256": _sha256_hex(output_text) if isinstance(output_text, str) else "",
                        "final_output_len": len(final_output),
                        "final_output_sha256": _sha256_hex(final_output),
                    },
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


class BridgeServer(socketserver.UnixStreamServer):
    allow_reuse_address = True

    def __init__(self, socket_path: str, model_path: str, model_id: str):
        if os.path.exists(socket_path):
            os.remove(socket_path)
        self.model_path = _normalize_path(model_path)
        self.model_id = model_id
        super().__init__(socket_path, BridgeRequestHandler)


def run_server(model_path: str, model_id: str, socket_path: str) -> int:
    normalized_path = _normalize_path(model_path)
    server = BridgeServer(socket_path=socket_path, model_path=normalized_path, model_id=model_id)
    try:
        model, tokenizer = load_model_and_tokenizer(normalized_path)
        _ = model
        loaded_identity = _derive_loaded_identity(normalized_path, model_id)
        print(
            json.dumps(
                {
                    "status": "ready",
                    "loaded_model_path": normalized_path,
                    "loaded_model_identity": loaded_identity,
                    "tokenizer_loaded": tokenizer is not None,
                }
            ),
            flush=True,
        )
        server.serve_forever()
        return 0
    finally:
        server.server_close()
        if os.path.exists(socket_path):
            os.remove(socket_path)


def run_once(model_path: str, model_id: str) -> int:
    normalized_path = _normalize_path(model_path)
    try:
        request = _read_json_line(sys.stdin)
    except Exception as exc:
        print(json.dumps({"error": f"invalid request json: {exc}"}), file=sys.stderr)
        return 1

    try:
        _, generate = load_generate_function()
        model, tokenizer = load_model_and_tokenizer(normalized_path)
        prompt = request.get("prompt", "")
        if not isinstance(prompt, str) or not prompt.strip():
            raise RuntimeError("prompt is empty")

        request_model_id = request.get("model_id")
        if request_model_id != model_id:
            raise RuntimeError(
                f"request model_id mismatch: expected {model_id}, got {request_model_id}"
            )

        loaded_identity = _derive_loaded_identity(normalized_path, model_id)
        if loaded_identity != model_id:
            raise RuntimeError(
                f"loaded model identity mismatch: expected {model_id}, got {loaded_identity}"
            )

        output_text = generate(
            model,
            tokenizer,
            prompt=prompt,
            verbose=False,
        )
        payload = {
            "request_id": request.get("request_id", "unknown"),
            "output_text": output_text.strip(),
            "finished": True,
            "loaded_model_path": normalized_path,
            "loaded_model_identity": loaded_identity,
            "tokenizer_loaded": tokenizer is not None,
        }
        print(json.dumps(payload), flush=True)
        return 0
    except Exception as exc:
        print(json.dumps({"error": str(exc)}), file=sys.stderr, flush=True)
        return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--model-id", required=True)
    parser.add_argument("--daemon", action="store_true")
    parser.add_argument(
        "--socket-path",
        default=os.path.join(tempfile.gettempdir(), "deterministic_ai_kernel_mlx.sock"),
    )
    args = parser.parse_args()

    if args.daemon:
        return run_server(model_path=args.model, model_id=args.model_id, socket_path=args.socket_path)
    return run_once(model_path=args.model, model_id=args.model_id)


if __name__ == "__main__":
    raise SystemExit(main())
