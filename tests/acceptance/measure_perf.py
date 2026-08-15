#!/usr/bin/env python3
"""R9 companion — §4 runtime/model performance harness.

Measures, against ANY OpenAI-compatible endpoint (no mocks):
  - TTFT: time from request start to first streamed chunk
  - wall: total wall time of the generation
  - tokens: generated tokens (usage if the server reports it, else delta
    pieces counted — approximation, labeled)
  - throughput: tokens/wall
  - rss_mb: server process RSS (pass --pid), before/after

Fixed probe set (short math, short RU, long reasoning, long context) so
stacks are comparable run-to-run. Logs every probe to CSV.

Usage:
  python3 tests/acceptance/measure_perf.py \
      --base-url http://127.0.0.1:8081/v1 \
      --model /Users/denissmoliakov/Models/gemma4-reasoning \
      --label mlx_gemma4_reasoning \
      [--pid <server_pid>] [--out /tmp/dek_ai_matrix/perf_<label>.csv]
"""
import argparse
import json
import time
import urllib.request

PROBES = [
    ("P1_short_math", "Сколько будет 17 умножить на 24?", 256),
    ("P2_short_ru", "Объясни одним предложением, что такое throughput.", 256),
    ("P3_long_reasoning", "Что такое мертлок в механике? Ответь кратко.", 600),
    (
        "P4_long_context",
        "Текст: " + ("Журнал показал нормальную работу насоса номер 9. " * 120)
        + " Вопрос: какой номер насоса упомянут в тексте? Ответь числом.",
        256,
    ),
]


def stream_request(base_url, model, payload, max_tokens):
    body = json.dumps(
        {
            "model": model,
            "messages": [{"role": "user", "content": payload}],
            "temperature": 0.0,
            "max_tokens": max_tokens,
            "stream": True,
        }
    ).encode()
    req = urllib.request.Request(
        base_url.rstrip("/") + "/chat/completions",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    t0 = time.time()
    ttft = None
    pieces = 0
    chars = 0
    usage_tokens = None
    buf = b""
    with urllib.request.urlopen(req, timeout=600) as resp:
        while True:
            chunk = resp.read(256)
            if not chunk:
                break
            now = time.time()
            if ttft is None:
                ttft = now - t0
            buf += chunk
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                line = line.strip()
                if not line.startswith(b"data:"):
                    continue
                data = line[5:].strip()
                if data == b"[DONE]":
                    continue
                try:
                    d = json.loads(data)
                except Exception:
                    continue
                u = d.get("usage")
                if isinstance(u, dict) and u.get("completion_tokens"):
                    usage_tokens = u["completion_tokens"]
                for ch in d.get("choices", []):
                    delta = ch.get("delta") or {}
                    for piece in (delta.get("content"), delta.get("reasoning")):
                        if piece:
                            pieces += 1
                            chars += len(piece)
    wall = time.time() - t0
    return {
        "ttft_s": round(ttft if ttft is not None else wall, 3),
        "wall_s": round(wall, 3),
        "delta_pieces": pieces,
        "chars": chars,
        "usage_tokens": usage_tokens,
    }


def rss_mb(pid):
    if not pid:
        return None
    try:
        import subprocess

        out = subprocess.run(
            ["ps", "-o", "rss=", "-p", str(pid)],
            capture_output=True,
            text=True,
            timeout=10,
        ).stdout.strip()
        return round(int(out) / 1024, 1) if out else None
    except Exception:
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base-url", required=True)
    ap.add_argument("--model", required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--pid", type=int, default=0)
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    out = args.out or f"/tmp/dek_ai_matrix/perf_{args.label}.csv"
    rows = []
    print(f"[{args.label}] endpoint={args.base_url} model={args.model}")
    for name, payload, maxtok in PROBES:
        t0 = time.time()
        try:
            r = stream_request(args.base_url, args.model, payload, maxtok)
            tokens = r["usage_tokens"] or r["delta_pieces"]
            tok_src = "usage" if r["usage_tokens"] else "delta_pieces(approx)"
            tps = round(tokens / r["wall_s"], 1) if r["wall_s"] > 0 else 0
            row = {
                "label": args.label,
                "probe": name,
                "ttft_s": r["ttft_s"],
                "wall_s": r["wall_s"],
                "tokens": tokens,
                "token_source": tok_src,
                "throughput_tps": tps,
                "rss_mb_after": rss_mb(args.pid),
                "ok": True,
            }
            print(
                f"  {name}: ttft={r['ttft_s']}s wall={r['wall_s']}s "
                f"tokens={tokens}({tok_src}) tps={tps}"
            )
        except Exception as e:
            row = {
                "label": args.label,
                "probe": name,
                "ttft_s": None,
                "wall_s": round(time.time() - t0, 3),
                "tokens": 0,
                "token_source": "error",
                "throughput_tps": 0,
                "rss_mb_after": rss_mb(args.pid),
                "ok": False,
                "error": str(e)[:120],
            }
            print(f"  {name}: ERROR {str(e)[:120]}")
        rows.append(row)

    with open(out, "w") as f:
        f.write(",".join(rows[0].keys()) + "\n")
        for r in rows:
            f.write(",".join(str(r.get(k, "")) for k in rows[0].keys()) + "\n")
    print(f"[{args.label}] csv={out}")


if __name__ == "__main__":
    main()
