#!/usr/bin/env python3
import argparse
import hashlib
import json
import platform
import subprocess
import sys
import time
from collections import defaultdict
from pathlib import Path

def node_key(node):
    return f'{node["id"]}@{node.get("version", "v1")}'

def parse_ref(ref):
    if "@" in ref:
        node_id, version = ref.split("@", 1)
        return node_id, version
    return ref, None

def command_output(argv):
    try:
        proc = subprocess.run(argv, capture_output=True, text=True, check=False)
        text = (proc.stdout or proc.stderr or "").strip()
        return text if text else "unknown"
    except Exception:
        return "unknown"

def stable_hash(payload):
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()

def collect_environment():
    strict = {
        "python": platform.python_version(),
        "cargo": command_output(["cargo", "--version"]),
        "rustc": command_output(["rustc", "--version"]),
    }
    debug = {
        "platform": platform.platform(),
        "system": platform.system(),
        "release": platform.release(),
        "machine": platform.machine(),
        "cwd": str(Path.cwd().resolve()),
    }
    environment_id = stable_hash(strict)
    env = dict(strict)
    env["environment_id"] = environment_id
    env["environment_fingerprint"] = environment_id
    return env, debug

def load_manifest(path):
    with open(path) as f:
        data = json.load(f)

    graph_schema_version = data.get("graph_schema_version", 1)
    default_pipeline = data.get("default_pipeline", "fast")
    pipelines = data.get("pipelines", {"fast": ["preflight", "contract"], "deep": ["preflight", "contract", "stress"]})
    deterministic_order = data.get("deterministic_order", "topological_then_priority_then_id_version")

    raw_nodes = data["nodes"]
    nodes = []
    by_key = {}
    versions_by_id = defaultdict(set)

    for raw in raw_nodes:
        n = dict(raw)
        n.setdefault("version", "v1")
        n.setdefault("stage", "contract")
        n.setdefault("pipeline", "fast")
        n.setdefault("priority", 100)
        n.setdefault("cost", 1)
        n["outputs"] = n.get("outputs", n.get("produces", []))
        key = node_key(n)
        if key in by_key:
            raise SystemExit(f"duplicate node key: {key}")
        nodes.append(n)
        by_key[key] = n
        versions_by_id[n["id"]].add(n["version"])

    return {
        "manifest": data,
        "graph_schema_version": graph_schema_version,
        "default_pipeline": default_pipeline,
        "pipelines": pipelines,
        "deterministic_order": deterministic_order,
        "nodes": nodes,
        "by_key": by_key,
        "versions_by_id": versions_by_id,
    }

def resolve_ref(ref, versions_by_id):
    node_id, version = parse_ref(ref)
    if version is not None:
        return f"{node_id}@{version}"
    versions = sorted(versions_by_id.get(node_id, []))
    if not versions:
        raise SystemExit(f"unknown node reference: {ref}")
    if len(versions) > 1:
        raise SystemExit(f"ambiguous node reference without version: {ref}")
    return f"{node_id}@{versions[0]}"

def selected_by_pipeline(nodes, pipeline_name, pipelines):
    allowed_stages = set(pipelines[pipeline_name])
    return {node_key(n) for n in nodes if n.get("stage") in allowed_stages}

def dependency_closure(initial_keys, by_key, versions_by_id):
    need = set()
    visiting = set()

    def visit(key):
        if key in need:
            return
        if key in visiting:
            raise SystemExit(f"cycle detected at {key}")
        if key not in by_key:
            raise SystemExit(f"unknown node key: {key}")
        visiting.add(key)
        for dep in by_key[key].get("depends_on", []):
            dep_key = resolve_ref(dep, versions_by_id)
            visit(dep_key)
        visiting.remove(key)
        need.add(key)

    for key in sorted(initial_keys):
        visit(key)
    return need

def canonical_order(selected_keys, by_key, versions_by_id):
    indegree = {k: 0 for k in selected_keys}
    edges = defaultdict(list)

    for key in selected_keys:
        for dep in by_key[key].get("depends_on", []):
            dep_key = resolve_ref(dep, versions_by_id)
            if dep_key in selected_keys:
                edges[dep_key].append(key)
                indegree[key] += 1

    ready = [k for k, v in indegree.items() if v == 0]
    ready.sort(key=lambda k: (by_key[k].get("priority", 100), k))

    ordered = []
    while ready:
        key = ready.pop(0)
        ordered.append(key)
        for nxt in sorted(edges[key]):
            indegree[nxt] -= 1
            if indegree[nxt] == 0:
                ready.append(nxt)
                ready.sort(key=lambda k: (by_key[k].get("priority", 100), k))

    if len(ordered) != len(selected_keys):
        raise SystemExit("cycle detected during canonical ordering")
    return ordered

def compute_spec_hash(ctx):
    manifest_canonical = json.dumps(ctx["manifest"], sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(manifest_canonical.encode("utf-8")).hexdigest()

def compute_execution_order_id(ctx, ordered_keys):
    return stable_hash({
        "deterministic_order": ctx["deterministic_order"],
        "ordered_keys": ordered_keys,
    })

def compute_plan_hash(ctx, args, ordered_keys, environment_id):
    spec_hash = compute_spec_hash(ctx)
    payload = {
        "spec_hash": spec_hash,
        "graph_schema_version": ctx["graph_schema_version"],
        "selected_pipeline": args.pipeline,
        "selected_only": [x.strip() for x in args.only.split(",") if x.strip()],
        "deterministic_order": ctx["deterministic_order"],
        "ordered_keys": ordered_keys,
        "environment_id": environment_id,
    }
    return stable_hash(payload)

def build_plan(ctx, args, environment, environment_debug):
    nodes = ctx["nodes"]
    by_key = ctx["by_key"]
    versions_by_id = ctx["versions_by_id"]
    pipelines = ctx["pipelines"]

    if args.pipeline not in pipelines:
        raise SystemExit(f"unknown pipeline: {args.pipeline}")

    pipeline_keys = selected_by_pipeline(nodes, args.pipeline, pipelines)

    only_refs = [x.strip() for x in args.only.split(",") if x.strip()]
    if only_refs:
        requested = {resolve_ref(ref, versions_by_id) for ref in only_refs}
        initial_keys = dependency_closure(requested, by_key, versions_by_id)
    else:
        initial_keys = dependency_closure(pipeline_keys, by_key, versions_by_id)

    ordered_keys = canonical_order(initial_keys, by_key, versions_by_id)
    environment_id = environment["environment_id"]
    execution_order_id = compute_execution_order_id(ctx, ordered_keys)
    plan_hash = compute_plan_hash(ctx, args, ordered_keys, environment_id)

    plan = {
        "ok": True,
        "plan_id": plan_hash,
        "plan_hash": plan_hash,
        "spec_hash": compute_spec_hash(ctx),
        "environment_id": environment_id,
        "execution_order": ordered_keys,
        "execution_order_id": execution_order_id,
        "graph_schema_version": ctx["graph_schema_version"],
        "default_pipeline": ctx["default_pipeline"],
        "selected_pipeline": args.pipeline,
        "selected_only": only_refs,
        "deterministic_order": ctx["deterministic_order"],
        "manifest_path": args.manifest,
        "plan_generated_at_epoch": int(time.time()),
        "environment": environment,
        "environment_debug": environment_debug,
        "ordered_nodes": [],
    }

    total_cost = 0
    for key in ordered_keys:
        node = by_key[key]
        total_cost += int(node.get("cost", 1))
        plan["ordered_nodes"].append({
            "key": key,
            "id": node["id"],
            "version": node["version"],
            "stage": node["stage"],
            "pipeline": node["pipeline"],
            "priority": node["priority"],
            "cost": node["cost"],
            "depends_on": [resolve_ref(dep, versions_by_id) for dep in node.get("depends_on", [])],
            "cmd": node["cmd"],
            "outputs": node.get("outputs", []),
        })

    plan["total_cost"] = total_cost
    return plan

def load_json(path):
    with open(path) as f:
        return json.load(f)

def write_verdict(verdict, verdict_out):
    verdict_path = Path(verdict_out)
    verdict_path.parent.mkdir(parents=True, exist_ok=True)
    verdict_path.write_text(json.dumps(verdict, indent=2) + "\n")
    print(f"verdict_file={verdict_path} bytes={verdict_path.stat().st_size}")
    print(json.dumps(verdict, indent=2))

def plan_environment_id(plan):
    return plan.get("environment_id") or plan.get("environment", {}).get("environment_id") or plan.get("environment", {}).get("environment_fingerprint")

def plan_execution_order_id(plan):
    return plan.get("execution_order_id")

def enforce_reuse_policy(current_plan, reuse_plan_path, verdict_out):
    prior = load_json(reuse_plan_path)

    prior_plan_id = prior.get("plan_id") or prior.get("plan_hash")
    current_plan_id = current_plan.get("plan_id")

    prior_plan_hash = prior.get("plan_hash") or prior.get("plan_id")
    current_plan_hash = current_plan.get("plan_hash")

    if prior_plan_hash == current_plan_hash:
        prior_fp = prior.get("environment_fingerprint") or prior.get("environment", {}).get("environment_fingerprint")
        current_fp = current_plan.get("environment_fingerprint") or current_plan.get("environment", {}).get("environment_fingerprint")
        if prior_fp != current_fp:
            verdict = {
                "ok": False,
                "status": "invalid_reuse",
                "reason": "plan_hash matched but environment_fingerprint differed",
                "plan_id": current_plan["plan_id"],
                "plan_hash": current_plan["plan_hash"],
                "environment_id": current_plan["environment_id"],
                "execution_order_id": current_plan["execution_order_id"],
                "expected_environment_fingerprint": prior_fp,
                "actual_environment_fingerprint": current_fp,
                "graph_schema_version": current_plan["graph_schema_version"],
                "selected_pipeline": current_plan["selected_pipeline"],
                "selected_only": current_plan["selected_only"],
                "deterministic_order": current_plan["deterministic_order"],
                "node_count": 0,
                "environment": current_plan["environment"],
                "environment_debug": current_plan["environment_debug"],
                "nodes": [],
            }
            write_verdict(verdict, verdict_out)
            raise SystemExit(2)
        return

    if prior_plan_id == current_plan_id:
        return

    prior_pipeline = prior.get("selected_pipeline")
    current_pipeline = current_plan.get("selected_pipeline")

    if prior_pipeline != current_pipeline:
        verdict = {
            "ok": False,
            "status": "invalid_reuse",
            "reason": "plan_id differed: selected_pipeline differed for reuse-plan",
            "plan_id": current_plan["plan_id"],
            "plan_hash": current_plan["plan_hash"],
            "environment_id": current_plan["environment_id"],
            "execution_order_id": current_plan["execution_order_id"],
            "expected_selected_pipeline": prior_pipeline,
            "actual_selected_pipeline": current_pipeline,
            "graph_schema_version": current_plan["graph_schema_version"],
            "selected_pipeline": current_plan["selected_pipeline"],
            "selected_only": current_plan["selected_only"],
            "deterministic_order": current_plan["deterministic_order"],
            "node_count": 0,
            "environment": current_plan["environment"],
            "environment_debug": current_plan["environment_debug"],
            "nodes": [],
        }
        write_verdict(verdict, verdict_out)
        raise SystemExit(2)

    prior_env = plan_environment_id(prior)
    current_env = plan_environment_id(current_plan)

    if prior_env != current_env:
        verdict = {
            "ok": False,
            "status": "invalid_reuse",
            "reason": "plan_id differed: environment_id differed",
            "plan_id": current_plan["plan_id"],
            "plan_hash": current_plan["plan_hash"],
            "environment_id": current_plan["environment_id"],
            "execution_order_id": current_plan["execution_order_id"],
            "expected_environment_id": prior_env,
            "actual_environment_id": current_env,
            "graph_schema_version": current_plan["graph_schema_version"],
            "selected_pipeline": current_plan["selected_pipeline"],
            "selected_only": current_plan["selected_only"],
            "deterministic_order": current_plan["deterministic_order"],
            "node_count": 0,
            "environment": current_plan["environment"],
            "environment_debug": current_plan["environment_debug"],
            "nodes": [],
        }
        write_verdict(verdict, verdict_out)
        raise SystemExit(2)

    prior_order = plan_execution_order_id(prior)
    current_order = plan_execution_order_id(current_plan)
    if prior_order != current_order:
        reason = "plan_id differed: execution_order_id differed"
    else:
        reason = "plan_id differed"

    verdict = {
        "ok": False,
        "status": "invalid_reuse",
        "reason": reason,
        "plan_id": current_plan["plan_id"],
        "plan_hash": current_plan["plan_hash"],
        "environment_id": current_plan["environment_id"],
        "execution_order_id": current_plan["execution_order_id"],
        "expected_plan_id": prior_plan_id,
        "actual_plan_id": current_plan_id,
        "graph_schema_version": current_plan["graph_schema_version"],
        "selected_pipeline": current_plan["selected_pipeline"],
        "selected_only": current_plan["selected_only"],
        "deterministic_order": current_plan["deterministic_order"],
        "node_count": 0,
        "environment": current_plan["environment"],
        "environment_debug": current_plan["environment_debug"],
        "nodes": [],
    }
    write_verdict(verdict, verdict_out)
    raise SystemExit(2)

def run_plan(plan, by_key, verdict_out):
    start = time.time()
    results = []

    for item in plan["ordered_nodes"]:
        key = item["key"]
        node = by_key[key]
        print(f"==> node={key}", flush=True)
        t0 = time.time()
        proc = subprocess.run(node["cmd"], shell=True)
        dt = round(time.time() - t0, 3)

        result = {
            "key": key,
            "id": node["id"],
            "version": node["version"],
            "stage": node["stage"],
            "pipeline": node["pipeline"],
            "status": "passed" if proc.returncode == 0 else "failed",
            "exit_code": proc.returncode,
            "seconds": dt,
            "priority": node["priority"],
            "cost": node["cost"],
            "depends_on": item["depends_on"],
            "outputs": node.get("outputs", []),
        }
        results.append(result)

        if proc.returncode != 0:
            break

    verdict = {
        "ok": all(r["status"] == "passed" for r in results),
        "status": "ok" if all(r["status"] == "passed" for r in results) else "failed",
        "plan_id": plan["plan_id"],
        "plan_hash": plan["plan_hash"],
        "environment_id": plan["environment_id"],
        "execution_order_id": plan["execution_order_id"],
        "environment_fingerprint": plan["environment"]["environment_fingerprint"],
        "graph_schema_version": plan["graph_schema_version"],
        "selected_pipeline": plan["selected_pipeline"],
        "selected_only": plan["selected_only"],
        "deterministic_order": plan["deterministic_order"],
        "graph_seconds": round(time.time() - start, 3),
        "node_count": len(results),
        "environment": plan["environment"],
        "environment_debug": plan["environment_debug"],
        "nodes": results,
    }

    write_verdict(verdict, verdict_out)
    return 0 if verdict["ok"] else 1

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default="docs/verification_graph.json")
    ap.add_argument("--pipeline", default=None)
    ap.add_argument("--only", default="")
    ap.add_argument("--plan-out", default="artifacts/verification_plan.json")
    ap.add_argument("--out", default="artifacts/verification_verdict.json")
    ap.add_argument("--reuse-plan", default="")
    args = ap.parse_args()

    ctx = load_manifest(args.manifest)
    if args.pipeline is None:
        args.pipeline = ctx["default_pipeline"]

    environment, environment_debug = collect_environment()
    plan = build_plan(ctx, args, environment, environment_debug)

    plan_path = Path(args.plan_out)
    plan_path.parent.mkdir(parents=True, exist_ok=True)
    plan_path.write_text(json.dumps(plan, indent=2) + "\n")
    print(f"plan_file={plan_path} bytes={plan_path.stat().st_size}")

    if args.reuse_plan:
        enforce_reuse_policy(plan, args.reuse_plan, args.out)

    sys.exit(run_plan(plan, ctx["by_key"], args.out))

if __name__ == "__main__":
    main()
