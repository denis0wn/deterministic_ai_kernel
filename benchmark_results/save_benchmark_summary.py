import csv, re, subprocess
from pathlib import Path
from datetime import datetime

root = Path("/Users/denissmoliakov/deterministic_ai_kernel")
outdir = root / "benchmark_results"
outdir.mkdir(exist_ok=True)

stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
raw = outdir / f"benchmark_raw_{stamp}.txt"
csv_path = outdir / "benchmark_summary.csv"
md_path = outdir / "latest_summary.md"

proc = subprocess.run(
    ["cargo", "test", "--test", "benchmark_llm_vs_kernel", "--", "--nocapture"],
    cwd=root,
    text=True,
    capture_output=True,
)
text = proc.stdout + proc.stderr
raw.write_text(text)

completed = len(re.findall(r"STEP_COMPLETED_BY_WORKER:", text))
failed = len(re.findall(r"STEP_FAILED_BY_WORKER:", text))
replay_valid = len(re.findall(r"REPLAY VALID", text))
committed_effects = [int(x) for x in re.findall(r"COMMITTED_EFFECTS:\s*(\d+)", text)]
rejected_effects = [int(x) for x in re.findall(r"REJECTED_EFFECTS:\s*(\d+)", text)]
rows = [int(x) for x in re.findall(r"EXECUTED_EFFECT_ROWS:\s*(\d+)", text)]
receipts = re.findall(r"DEBUG RECEIPT: status=(\w+), completed_steps=(\d+), failed_attempts=(\d+), retry_count=(\d+)", text)

runs = len(receipts)
statuses = [r[0] for r in receipts]
completed_steps = [int(r[1]) for r in receipts]
failed_attempts = [int(r[2]) for r in receipts]
retry_counts = [int(r[3]) for r in receipts]
replay_checked_runs = runs
replay_valid_runs = min(replay_valid, replay_checked_runs)

avg = lambda xs: round(sum(xs) / len(xs), 2) if xs else 0.0
completion_rate = round((sum(1 for s in statuses if s == "completed") / runs * 100.0), 2) if runs else 0.0
replay_validation_rate = round((replay_valid_runs / replay_checked_runs * 100.0), 2) if replay_checked_runs else 0.0
step_failure_rate = round((failed / (completed + failed) * 100.0), 2) if (completed + failed) else 0.0

header = [
    "timestamp","runs","completion_rate_pct","replay_validation_rate_pct","avg_completed_steps",
    "avg_failed_attempts","avg_retry_count","avg_executed_effect_rows",
    "avg_committed_effects","avg_rejected_effects","step_failure_rate_pct","exit_code","raw_file"
]
row = [
    stamp, runs, completion_rate, replay_validation_rate, avg(completed_steps),
    avg(failed_attempts), avg(retry_counts), avg(rows),
    avg(committed_effects), avg(rejected_effects), step_failure_rate, proc.returncode, raw.name
]

write_header = not csv_path.exists()
with csv_path.open("a", newline="") as f:
    w = csv.writer(f)
    if write_header:
        w.writerow(header)
    w.writerow(row)

md = f"""# Benchmark summary

| Metric | Value |
|---|---:|
| Timestamp | {stamp} |
| Runs | {runs} |
| Completion rate | {completion_rate:.2f}% |
| Replay validation rate | {replay_validation_rate:.2f}% |
| Avg completed steps | {avg(completed_steps):.2f} |
| Avg failed attempts | {avg(failed_attempts):.2f} |
| Avg retry count | {avg(retry_counts):.2f} |
| Avg executed effect rows | {avg(rows):.2f} |
| Avg committed effects | {avg(committed_effects):.2f} |
| Avg rejected effects | {avg(rejected_effects):.2f} |
| Step failure rate | {step_failure_rate:.2f}% |
| Exit code | {proc.returncode} |
| Raw log | `{raw.name}` |
"""
md_path.write_text(md)

print(md)
