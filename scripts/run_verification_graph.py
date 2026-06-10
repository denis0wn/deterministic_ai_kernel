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

def collect_environment():
    env = {
        "python": platform.python_version(),
        "cargo": command_output(["cargo", "--version"]),
        "rustc": command_output(["rustc", "--version"]),
        "platform": platform.platform(),
        "system": platform.system(),
        "release": platform.release(),
        "machine": platform.machine(),
        "cwd": str(Path.cwd().resolve()),
    }
    canonical = json.dumps(env, sort_keys=True, separators=(",", ":"))
    env["environment_fingerprint"] = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    return env

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

def compute_plan_hash(ctx, args, ordered_keys, environment):
    manifest_canonical = json.dumps(ctx["manifest"], sort_keys=True, separators=(",", ":"))
    payload = {
        "manifest": json.loads(manifest_canonical),
        "graph_schema_version": ctx["graph_schema_version"],
        "selected_pipeline": args.pipeline,
        "selected_only": [x.strip() for x in args.only.split(",") if x.strip()],
        "deterministic_order": ctx["deterministic_order"],
        "ordered_keys": ordered_keys,
        "environment_fingerprint": environment["environment_fingerprint"],
    }
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()

def build_plan(ctx, args, environment):
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
    plan_hash = compute_plan_hash(ctx, args, ordered_keys, environment)

    plan = {
        "ok": True,
        "plan_hash": plan_hash,
        "graph_schema_version": ctx["graph_schema_version"],
        "default_pipeline": ctx["default_pipeline"],
        "selected_pipeline": args.pipeline,
        "selected_only": only_refs,
        "deterministic_order": ctx["deterministic_order"],
        "manifest_path": args.manifest,
        "plan_generated_at_epoch": int(time.time()),
        "environment": environment,
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

def enforce_reuse_policy(current_plan, reuse_plan_path, verdict_out):
    prior = load_json(reuse_plan_path)

    prior_pipeline = prior.get("selected_pipeline")
    current_pipeline = current_plan.get("selected_pipeline")

    if prior_pipeline != current_pipeline:
        verdict = {
            "ok": False,
            "status": "invalid_reuse",
            "reason": "selected_pipeline differed for reuse-plan",
            "plan_hash": current_plan["plan_hash"],
            "expected_selected_pipeline": prior_pipeline,
            "actual_selected_pipeline": current_pipeline,
            "graph_schema_version": current_plan["graph_schema_version"],
            "selected_pipeline": current_plan["selected_pipeline"],
            "selected_only": current_plan["selected_only"],
            "deterministic_order": current_plan["deterministic_order"],
            "node_count": 0,
            "environment": current_plan["environment"],
            "nodes": [],
        }
        write_verdict(verdict, verdict_out)
        raise SystemExit(2)

    same_plan_hash = prior.get("plan_hash") == current_plan.get("plan_hash")
    same_env = (
        prior.get("environment", {}).get("environment_fingerprint")
        == current_plan.get("environment", {}).get("environment_fingerprint")
    )

    if same_plan_hash and not same_env:
        verdict = {
            "ok": False,
            "status": "invalid_reuse",
            "reason": "plan_hash matched but environment_fingerprint differed",
            "plan_hash": current_plan["plan_hash"],
            "expected_environment_fingerprint": prior.get("environment", {}).get("environment_fingerprint"),
            "actual_environment_fingerprint": current_plan.get("environment", {}).get("environment_fingerprint"),
            "graph_schema_version": current_plan["graph_schema_version"],
            "selected_pipeline": current_plan["selected_pipeline"],
            "selected_only": current_plan["selected_only"],
            "deterministic_order": current_plan["deterministic_order"],
            "node_count": 0,
            "environment": current_plan["environment"],
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
        "plan_hash": plan["plan_hash"],
        "environment_fingerprint": plan["environment"]["environment_fingerprint"],
        "graph_schema_version": plan["graph_schema_version"],
        "selected_pipeline": plan["selected_pipeline"],
        "selected_only": plan["selected_only"],
        "deterministic_order": plan["deterministic_order"],
        "graph_seconds": round(time.time() - start, 3),
        "node_count": len(results),
        "environment": plan["environment"],
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

    environment = collect_environment()
    plan = build_plan(ctx, args, environment)

    plan_path = Path(args.plan_out)
    plan_path.parent.mkdir(parents=True, exist_ok=True)
    plan_path.write_text(json.dumps(plan, indent=2) + "\n")
    print(f"plan_file={plan_path} bytes={plan_path.stat().st_size}")

    if args.reuse_plan:
        enforce_reuse_policy(plan, args.reuse_plan, args.out)

    sys.exit(run_plan(plan, ctx["by_key"], args.out))

if __name__ == "__main__":
    main()
