use crate::execution::patch_contract;
use crate::execution::patch_repair;
use crate::execution_abi::primitives::{
    ArtifactSpec, PrimitiveKind, PrimitiveResult, PrimitiveSpec,
};
use crate::providers;
use anyhow::{anyhow, Result};
use serde_json::json;

/// Cached result of embeddings endpoint probe (process-lifetime).
/// Parse verifier verdict from LLM response text.
/// Accepts JSON with "verdict" field, or plain "PASS"/"FAIL" text.
///
/// Fail-closed contract (audit findings M4/EH2): only an explicit PASS opens
/// the gate. Unparseable, ambiguous, or malformed verifier responses are
/// treated as FAIL — a verification failure must never silently become PASS.
fn parse_verdict(text: &str) -> String {
    fn normalize(verdict: &str) -> String {
        if verdict.eq_ignore_ascii_case("pass") {
            "PASS".to_string()
        } else {
            "FAIL".to_string()
        }
    }

    // Try JSON parse first
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(verdict) = v.get("verdict").and_then(|v| v.as_str()) {
            return normalize(verdict);
        }
    }
    // Try to extract JSON from markdown fences
    let cleaned = crate::llm::extract_json(text);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cleaned) {
        if let Some(verdict) = v.get("verdict").and_then(|v| v.as_str()) {
            return normalize(verdict);
        }
    }
    // Fallback: check for plain text PASS/FAIL
    let upper = text.to_uppercase();
    if upper.contains("PASS") && !upper.contains("FAIL") {
        return "PASS".to_string();
    }
    "FAIL".to_string()
}

pub struct PrimitiveExecutor;

/// Build the per-step LLM prompt.
///
/// AnswerQuestion steps dispatch to the QA prompt (P0, H-2 fix): questions
/// are answered directly instead of being framed as code-execution tasks.
/// Every other step kind keeps the code-executor frame.
fn executor_prompt_for(step_kind: &str, detail: &str) -> String {
    if step_kind == "AnswerQuestion" {
        format!("{}\n\nQUESTION: {}", crate::llm::QA_PROMPT, detail)
    } else {
        format!(
            "{}\n\nStep: {}\nDetail: {}",
            crate::llm::EXECUTOR_PROMPT,
            step_kind,
            detail
        )
    }
}

/// Build the grounding section with the real target file content for
/// LocateBug/PatchCode prompts (P1, H-1 fix). Empty string when the task
/// text does not name an existing file.
fn grounded_file_section(task_payload: &str) -> String {
    match patch_contract::extract_target_file(task_payload)
        .and_then(|target| std::fs::read_to_string(&target).ok().map(|c| (target, c)))
    {
        Some((target, content)) => {
            format!("\n\nFILE: {}\nFILE CONTENT:\n<<<\n{}\n>>>", target, content)
        }
        None => String::new(),
    }
}

/// Fill the PATCH_PROMPT placeholders (executor-owned substitution).
fn patch_prompt(target: &str, content: &str, task: &str) -> String {
    crate::llm::PATCH_PROMPT
        .replace("{target}", target)
        .replace("{content}", content)
        .replace("{task}", task)
}

/// Request a patch_v1 object from the LLM with one bounded repair retry
/// (same posture as llm::chat_structured). Unparseable output after the
/// retry is a terminal contract violation ("fatal: malformed patch").
///
/// S3 escape repair (stage 3): when `file_content` is provided and the
/// parse failure is an invalid-escape corruption, a deterministic
/// ground-truth-validated repair (patch_repair) is attempted BEFORE the
/// model retry — deterministic regeneration at temp 0 provably
/// reproduces the same corruption, so the model retry cannot help this
/// class. A successful repair returns its RepairReport for the audit
/// record (D5). The fallback order and all downstream gates are
/// unchanged (D3/D6).
fn request_patch_v1(
    prompt: &str,
    file_content: Option<&str>,
) -> Result<(patch_contract::PatchV1, Option<patch_repair::RepairReport>)> {
    let first = providers::get_llm().execute_llm(prompt, None)?.text;
    match patch_contract::parse_patch(&first) {
        Ok(patch) => Ok((patch, None)),
        Err(first_error) => {
            if patch_repair::repair_enabled() {
                if let Some(content) = file_content {
                    let extracted = crate::llm::extract_json(&first);
                    match patch_repair::repair_patch_json(&extracted, content) {
                        Ok((patch, report)) => return Ok((patch, Some(report))),
                        Err(reason) => {
                            eprintln!(
                                "patch_repair: deterministic repair not applied ({reason}); falling back to model repair retry"
                            );
                        }
                    }
                }
            }
            let repair_prompt = format!(
                "{}\n\nYour previous reply violated the patch_v1 contract: {}\nReply with ONLY the corrected JSON object.",
                prompt, first_error
            );
            let second = providers::get_llm().execute_llm(&repair_prompt, None)?.text;
            patch_contract::parse_patch(&second)
                .map(|p| (p, None))
                .map_err(|e| anyhow!("fatal: malformed patch: {e}"))
        }
    }
}

/// ApplyPatch dispatch (P2). Kernel-only effect: no LLM call. The validated
/// patch artifact is injected by the effect loop from the canonical artifact
/// store; workspace authorization comes from kernel-owned spec payload or
/// the operator env. Application runs exclusively through
/// `tools::registry::execute_tool`, the single enforcement boundary.
fn execute_apply_patch(
    spec: &PrimitiveSpec,
    payload: &serde_json::Value,
) -> Result<PrimitiveResult> {
    let patch_value = payload.get("patch_v1").ok_or_else(|| {
        anyhow!(
            "fatal: apply_patch step has no patch_v1 artifact injected (PatchCode must run first)"
        )
    })?;
    let patch: patch_contract::PatchV1 = serde_json::from_value(patch_value.clone())
        .map_err(|e| anyhow!("fatal: apply_patch step received invalid patch_v1: {e}"))?;

    let workspace = payload
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| std::env::var("DAK_CODEFIX_WORKSPACE").ok())
        .ok_or_else(|| {
            anyhow!(
                "fatal: apply_patch step has no authorized workspace (set DAK_CODEFIX_WORKSPACE)"
            )
        })?;

    let args = json!({ "patch": serde_json::to_value(&patch).expect("PatchV1 serializes") });
    // confirmed=true: kernel policy authorization. The patch has already
    // passed kernel-side validation (P1) and application is the authorized
    // effect of this step — the confirmation is owned by the kernel, never
    // by the LLM or an unattended UI.
    let tool_result = run_tool_sync("apply_patch_v1", &args, &workspace, true);
    if !tool_result.success {
        return Err(anyhow!(
            "fatal: patch application failed: {}",
            tool_result
                .error
                .unwrap_or_else(|| "unknown tool error".to_string())
        ));
    }

    let mut output = tool_result.output;
    if let Some(obj) = output.as_object_mut() {
        obj.insert("step_id".to_string(), json!(spec.id.0));
    }
    Ok(PrimitiveResult {
        id: spec.id.clone(),
        status: "ok".to_string(),
        output,
        artifacts: vec![],
    })
}

/// Run an async tool through the canonical gate from synchronous executor
/// context. A scoped thread owns a dedicated current-thread runtime, so this
/// works with or without an ambient tokio runtime.
fn run_tool_sync(
    name: &str,
    args: &serde_json::Value,
    workspace: &str,
    confirmed: bool,
) -> crate::tools::contract::ToolResult {
    let name = name.to_string();
    let args = args.clone();
    let workspace = workspace.to_string();
    std::thread::scope(|s| {
        let handle = s.spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tool runtime must build");
            rt.block_on(crate::tools::registry::execute_tool(
                &name, &args, &workspace, confirmed,
            ))
        });
        handle.join().expect("tool thread panicked")
    })
}

/// P3: REAL authorized test execution for the RunTests step.
///
/// The workspace comes from the kernel-injected payload or the operator env;
/// the test command itself is derived by the kernel from workspace
/// inspection inside the `run_tests_v1` tool — no LLM output is ever used
/// to build or select a command. A report whose `passed` is false (non-zero
/// exit, timeout, or unavailable command) fails the step terminally so a
/// CodeFix task can never complete on unproven or failing tests.
fn execute_run_tests(spec: &PrimitiveSpec, payload: &serde_json::Value) -> Result<PrimitiveResult> {
    let workspace = payload
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| std::env::var("DAK_CODEFIX_WORKSPACE").ok())
        .ok_or_else(|| {
            anyhow!("fatal: run_tests step has no authorized workspace (set DAK_CODEFIX_WORKSPACE)")
        })?;

    let timeout_secs = payload
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(crate::tools::test_runner::DEFAULT_TEST_TIMEOUT_SECS);

    let args = json!({ "timeout_secs": timeout_secs });
    // confirmed=true: kernel policy authorization for this CodeFix step.
    let tool_result = run_tool_sync("run_tests_v1", &args, &workspace, true);
    if !tool_result.success {
        return Err(anyhow!(
            "fatal: real test execution failed: {}",
            tool_result
                .error
                .unwrap_or_else(|| "unknown tool error".to_string())
        ));
    }

    let report = tool_result.output;
    let passed = report
        .get("passed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let exit_code = report
        .get("exit_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);

    let mut output = json!({
        "test_report_v1": report,
        "tests_passed": passed,
    });
    if let Some(obj) = output.as_object_mut() {
        obj.insert("step_id".to_string(), json!(spec.id.0));
    }

    if !passed {
        // Persist nothing false: surface the truthful failure. The kernel
        // never declares tests passed on the model's word — only on the
        // real exit status captured in test_report_v1. P4-B: the message
        // carries the kernel-owned classification so test failures,
        // timeouts, spawn failures and infrastructure errors stay
        // distinguishable end-to-end (no LLM interpretation anywhere).
        let cls = report
            .get("classification")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        use crate::tools::test_runner::outcome;
        return Err(match cls {
            outcome::TIMEOUT => anyhow!(
                "fatal: real tests timed out after {}s (classification={cls})",
                timeout_secs
            ),
            outcome::TESTS_FAILED => anyhow!(
                "fatal: real tests failed with exit code {} (classification={cls}; no fabricated success)",
                exit_code
            ),
            _ => anyhow!(
                "fatal: real test execution error: {cls} (exit={exit_code}; no fabricated success)"
            ),
        });
    }

    Ok(PrimitiveResult {
        id: spec.id.clone(),
        status: "ok".to_string(),
        output,
        artifacts: vec![],
    })
}

impl PrimitiveExecutor {
    pub fn execute(
        task_id: &str,
        spec: &PrimitiveSpec,
        task_payload: &str,
    ) -> Result<PrimitiveResult> {
        let payload = &spec.payload;

        match spec.kind {
            PrimitiveKind::Read => {
                let path = payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("repository");

                // P1 grounding (H-1 fix): "repository" reads resolve the real
                // target file named in the task payload when one exists;
                // otherwise the legacy payload fallback is preserved.
                let (resolved_path, content) = if path == "repository" {
                    if let Some(target) = patch_contract::extract_target_file(task_payload) {
                        if let Ok(c) = std::fs::read_to_string(&target) {
                            (target, c)
                        } else {
                            ("repository".to_string(), task_payload.to_string())
                        }
                    } else {
                        ("repository".to_string(), task_payload.to_string())
                    }
                } else if std::path::Path::new(path).exists() {
                    // Read actual file from filesystem
                    (
                        path.to_string(),
                        providers::get_filesystem().read_to_string(path)?,
                    )
                } else {
                    // File not found — return error context
                    (path.to_string(), format!("[file not found: {}]", path))
                };

                Ok(PrimitiveResult {
                    id: spec.id.clone(),
                    status: "ok".to_string(),
                    output: json!({
                        "path": resolved_path,
                        "content_len": content.len(),
                        "content": content
                    }),
                    artifacts: vec![],
                })
            }
            PrimitiveKind::Write => {
                let path = payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("artifacts/final_patch.txt");

                let requires_llm = payload
                    .get("requires_llm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let step_kind = payload
                    .get("step_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let mut extra_output = serde_json::Map::new();
                let content = if requires_llm && step_kind == "PatchCode" {
                    // P1 structured CodeFix contract (H-1 fix): the model must
                    // produce a patch_v1 object grounded in the REAL target
                    // file; the kernel validates shape, target identity and
                    // context uniqueness. Any violation is terminal ("fatal:")
                    // so a malformed or hallucinated patch can never ride a
                    // plausible-looking lifecycle into task completion.
                    let target = payload
                        .get("target_file")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .or_else(|| patch_contract::extract_target_file(task_payload))
                        .ok_or_else(|| {
                            anyhow!("fatal: malformed patch: no target file resolvable from task")
                        })?;
                    let file_content = std::fs::read_to_string(&target).map_err(|_| {
                        anyhow!(
                            "fatal: malformed patch: target file '{}' does not exist or is unreadable",
                            target
                        )
                    })?;
                    let prompt = patch_prompt(&target, &file_content, task_payload);
                    // Bounded target-correction: the initial attempt plus AT
                    // MOST ONE corrective retry when the model returns the
                    // wrong target_file. Every shape/context/target gate
                    // below still applies unchanged to the final attempt,
                    // and a persistent mismatch stays terminal — no gate is
                    // weakened, the model is only given one explicit
                    // correction chance.
                    let (patch, repair_report) = {
                        let (first, repair_first) = request_patch_v1(&prompt, Some(&file_content))?;
                        if first.target_file == target {
                            (first, repair_first)
                        } else {
                            let corrective = format!(
                                "{}\n\nYour previous reply used the WRONG target_file: \"{}\". The target_file MUST be EXACTLY \"{}\" (same letters, same case). Reply with ONLY the corrected JSON object, changing ONLY the target_file field.",
                                prompt, first.target_file, target
                            );
                            let (second, repair_second) =
                                request_patch_v1(&corrective, Some(&file_content))?;
                            (second, repair_second.or(repair_first))
                        }
                    };
                    patch_contract::validate_patch_shape(&patch)
                        .map_err(|e| anyhow!("fatal: malformed patch: {e}"))?;
                    if patch.target_file != target {
                        return Err(anyhow!(
                            "fatal: malformed patch: target mismatch: model proposed '{}' but task target is '{}'",
                            patch.target_file,
                            target
                        ));
                    }
                    let occurrences =
                        patch_contract::validate_patch_against_content(&patch, &file_content)
                            .map_err(|e| anyhow!("fatal: malformed patch: {e}"))?;
                    let canonical = serde_json::to_string_pretty(&patch)
                        .map_err(|e| anyhow!("fatal: malformed patch: serialization error: {e}"))?;
                    extra_output.insert(
                        "patch_v1".to_string(),
                        serde_json::to_value(&patch).expect("PatchV1 serializes"),
                    );
                    extra_output.insert("patch_target".to_string(), serde_json::json!(target));
                    extra_output.insert(
                        "context_occurrences".to_string(),
                        serde_json::json!(occurrences),
                    );
                    extra_output.insert(
                        "patch_shape_validation".to_string(),
                        serde_json::json!("ok"),
                    );
                    // D5 audit record: an applied S3 escape repair is
                    // never invisible — sites plus raw/repaired hashes
                    // ride the step artifact lineage.
                    if let Some(rep) = &repair_report {
                        extra_output.insert(
                            "patch_repair".to_string(),
                            serde_json::json!({
                                "repaired": true,
                                "mechanism": "escape_repair/R1",
                                "sites": rep.sites,
                                "raw_output_blake3": rep.raw_blake3,
                                "repaired_blake3": rep.repaired_blake3,
                            }),
                        );
                    }
                    canonical
                } else if requires_llm {
                    // Non-CodeFix LLM writes keep the legacy free-text path.
                    let prompt = format!(
                        "{}\n\nTask: {}\nStep: {}\nFile to patch: {}",
                        crate::llm::EXECUTOR_PROMPT,
                        task_payload,
                        spec.id.0,
                        path
                    );
                    let llm_res = providers::get_llm().execute_llm(&prompt, None)?;
                    llm_res.text
                } else {
                    payload
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or(task_payload)
                        .to_string()
                };

                providers::get_filesystem().write(path, &content)?;

                let mut output = serde_json::Map::new();
                output.insert("path".to_string(), serde_json::json!(path));
                output.insert(
                    "written_bytes".to_string(),
                    serde_json::json!(content.len()),
                );
                output.insert(
                    "content_preview".to_string(),
                    serde_json::json!(content.chars().take(200).collect::<String>()),
                );
                output.extend(extra_output);

                Ok(PrimitiveResult {
                    id: spec.id.clone(),
                    status: "ok".to_string(),
                    output: serde_json::Value::Object(output),
                    artifacts: vec![],
                })
            }
            PrimitiveKind::Compute => {
                // P2: ApplyPatch is a kernel-only effect step. It never
                // invokes the LLM: the patch artifact was generated and
                // validated by earlier steps, and application goes through
                // the authorized tool boundary only.
                let apply_kind = payload
                    .get("step_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if apply_kind == "ApplyPatch" {
                    return execute_apply_patch(spec, payload);
                }
                // P3: RunTests is REAL authorized test execution. The
                // command is kernel-derived from workspace inspection (never
                // LLM text), executed through the tool boundary with a hard
                // timeout, and a failing report fails the step terminally.
                if apply_kind == "RunTests" {
                    return execute_run_tests(spec, payload);
                }

                let operation = payload
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("none");
                let detail = payload
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .unwrap_or(task_payload);

                if matches!(operation, "semantic_embedding") {
                    // Provider-first: ask the registered LLM provider for an
                    // embedding. The previous implementation probed
                    // OPENAI_BASE_URL directly via curl, bypassing the
                    // provider abstraction and coupling the kernel to a live
                    // HTTP endpoint (audit finding: environment-dependent
                    // behavior/tests). The chat fallback now triggers only
                    // when the provider itself cannot produce an embedding.
                    match providers::get_llm().embed_text(detail) {
                        Ok(vector) => {
                            let artifact_payload = json!({
                                "input_representation": detail,
                                "embedding_dim": vector.len(),
                                "analysis_kind": "semantic_seed"
                            });
                            Ok(PrimitiveResult {
                                id: spec.id.clone(),
                                status: "ok".to_string(),
                                output: json!({
                                    "vector_len": vector.len(),
                                    "input_representation": detail,
                                    "analysis_kind": "semantic_seed"
                                }),
                                artifacts: vec![ArtifactSpec {
                                    artifact_type: "analysis_seed".to_string(),
                                    payload: artifact_payload,
                                }],
                            })
                        }
                        Err(_) => {
                            // Embeddings not supported by the provider — use chat/completions fallback.
                            let prompt = format!(
                            "Analyze the following task and return a brief structured summary:\n{}",
                            detail
                        );
                            let llm_res = providers::get_llm().execute_llm(&prompt, None)?;
                            let artifact_payload = json!({
                                "input_representation": detail,
                                "analysis_kind": "semantic_seed",
                                "fallback": "chat_completions",
                                "note": "embeddings endpoint unavailable, used chat/completions"
                            });
                            Ok(PrimitiveResult {
                                id: spec.id.clone(),
                                status: "ok".to_string(),
                                output: json!({
                                    "result": llm_res.text,
                                    "model_name": llm_res.model_name,
                                    "analysis_kind": "semantic_seed",
                                    "fallback": "chat_completions"
                                }),
                                artifacts: vec![ArtifactSpec {
                                    artifact_type: "analysis_seed".to_string(),
                                    payload: artifact_payload,
                                }],
                            })
                        }
                    }
                } else {
                    let requires_llm = payload
                        .get("requires_llm")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let output_val = if requires_llm {
                        let step_kind = payload
                            .get("step_kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("ExecuteChanges");
                        let mut prompt = executor_prompt_for(step_kind, detail);
                        if step_kind == "LocateBug" {
                            // P1 grounding (H-1 fix): the locator sees the
                            // real file content, not only the task wording.
                            prompt.push_str(&grounded_file_section(task_payload));
                        }

                        // R8: deterministic RAG for AnswerQuestion. Enabled
                        // only when DAK_RAG_DIR points at a non-empty KB
                        // dir; otherwise behavior is byte-identical to
                        // pre-R8. Retrieved documents enter the prompt as
                        // marked untrusted DATA (prompt-injection defense),
                        // and the grounding gate below checks claims
                        // against payload + retrieved text (provenance).
                        let rag = if step_kind == "AnswerQuestion" {
                            crate::rag::active_context(task_payload)
                        } else {
                            None
                        };
                        if let Some(rag_ctx) = &rag {
                            prompt.push_str(&rag_ctx.section);
                        }

                        let llm_res = providers::get_llm().execute_llm(&prompt, None)?;

                        // HD-2 hardening: kernel-owned grounding gate for
                        // AnswerQuestion. Identifier/measurement claims
                        // (serial IDs, RPM values) must appear LITERALLY in
                        // the task context, otherwise the answer must carry
                        // an explicit refusal. A fabricated fact is a
                        // TERMINAL failure — the kernel never records an
                        // ungrounded claim as a completed answer. Pattern
                        // matching only; the model is never re-consulted.
                        // R8: the grounding context is payload + retrieved
                        // documents (provenance grounding).
                        let (grounding_claims, grounding_refusal) = if step_kind == "AnswerQuestion"
                        {
                            let grounding_context = match &rag {
                                Some(r) if !r.retrieved_text.is_empty() => {
                                    format!("{}\n{}", task_payload, r.retrieved_text)
                                }
                                _ => task_payload.to_string(),
                            };
                            let claims = crate::grounding::find_unverified_claims(
                                &grounding_context,
                                &llm_res.text,
                            );
                            let refusal = crate::grounding::has_refusal_marker(&llm_res.text);
                            if !claims.is_empty() && !refusal {
                                let list = claims
                                    .iter()
                                    .map(|c| format!("{}:{}", c.kind, c.literal))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                return Err(anyhow!(
                                    "fatal: grounding violation (unverified_claim): \
                                         answer asserts facts absent from task context: \
                                         [{list}]; expected an explicit refusal"
                                ));
                            }
                            (claims, refusal)
                        } else {
                            (Vec::new(), false)
                        };

                        json!({
                            "result": llm_res.text,
                            "model_name": llm_res.model_name,
                            "model_version": llm_res.model_version,
                            "rag": {
                                "enabled": rag.is_some(),
                                "index_hash": rag.as_ref().map(|r| r.index_hash.clone()),
                                "retrieved": rag
                                    .as_ref()
                                    .map(|r| r.hits.iter().map(|h| h.doc_id.clone()).collect::<Vec<_>>())
                                    .unwrap_or_default(),
                                "policy": rag.as_ref().map(|r| r.policy).unwrap_or("disabled")
                            },
                            "grounding": {
                                "checked": step_kind == "AnswerQuestion",
                                "unverified_claims": serde_json::to_value(&grounding_claims)
                                    .unwrap_or(serde_json::json!([])),
                                "refusal_marker": grounding_refusal,
                                "verdict": if step_kind != "AnswerQuestion" {
                                    "not_applicable"
                                } else if !grounding_claims.is_empty() {
                                    "refusal_with_claims"
                                } else {
                                    "grounded"
                                }
                            }
                        })
                    } else {
                        json!({
                            "status": "computed",
                            "task_id": task_id
                        })
                    };

                    Ok(PrimitiveResult {
                        id: spec.id.clone(),
                        status: "ok".to_string(),
                        output: output_val,
                        artifacts: vec![],
                    })
                }
            }
            PrimitiveKind::Route => {
                let requires_llm = payload
                    .get("requires_llm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let step_kind = payload
                    .get("step_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // P3 completion gate for CodeFix: ValidatePatch is a
                // KERNEL-OWNED deterministic decision over real evidence —
                // the model never declares tests passed. Completion is only
                // allowed when (1) the patch was verified applied and (2)
                // real tests actually ran and exited 0. Both facts come from
                // canonical artifacts injected by the effect loop, never from
                // LLM text.
                if step_kind == "ValidatePatch" {
                    let apply_evidence = payload.get("apply_evidence");
                    let applied = apply_evidence
                        .and_then(|e| e.get("status"))
                        .and_then(|s| s.as_str())
                        == Some("applied")
                        && apply_evidence
                            .and_then(|e| e.get("evidence"))
                            .map(|e| e.is_object())
                            .unwrap_or(false);
                    if !applied {
                        return Err(anyhow!(
                            "fatal: validation gate failed: no verified patch-apply evidence (ApplyPatch must complete first)"
                        ));
                    }
                    let test_report = payload.get("test_report_v1");
                    let tests_passed = test_report
                        .and_then(|r| r.get("passed"))
                        .and_then(|p| p.as_bool())
                        .unwrap_or(false);
                    if !tests_passed {
                        return Err(anyhow!(
                            "fatal: validation gate failed: real tests did not pass (missing or failing test_report_v1)"
                        ));
                    }
                    return Ok(PrimitiveResult {
                        id: spec.id.clone(),
                        status: "ok".to_string(),
                        output: json!({
                            "route_decision": "PASS",
                            "gate": "deterministic_evidence",
                            "apply_status": "applied",
                            "tests_passed": true,
                            "test_command_id": test_report
                                .and_then(|r| r.get("command_id"))
                                .cloned()
                                .unwrap_or(json!(null)),
                            "test_exit_code": test_report
                                .and_then(|r| r.get("exit_code"))
                                .cloned()
                                .unwrap_or(json!(null))
                        }),
                        artifacts: vec![],
                    });
                }

                let route_decision = if requires_llm {
                    // Verifier gate: use structured prompt for validation
                    let prompt = format!(
                        "Verify the following output against the expected contract.\n\nTask: {}\nStep: {}\nOutput to verify:\n{}",
                        task_payload, spec.id.0,
                        payload.get("output").and_then(|v| v.as_str()).unwrap_or("no output provided")
                    );
                    let llm_res = providers::get_llm().execute_llm(
                        &format!("{}\n\n{}", crate::llm::VERIFIER_PROMPT, prompt),
                        None,
                    )?;
                    // Parse verifier response
                    parse_verdict(&llm_res.text)
                } else {
                    "Success".to_string()
                };

                Ok(PrimitiveResult {
                    id: spec.id.clone(),
                    status: "ok".to_string(),
                    output: json!({
                        "route_decision": route_decision
                    }),
                    artifacts: vec![],
                })
            }
            PrimitiveKind::Wait => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "ok".to_string(),
                output: json!({ "waited": true }),
                artifacts: vec![],
            }),
            PrimitiveKind::Signal => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "ok".to_string(),
                output: json!({ "signalled": true }),
                artifacts: vec![],
            }),
            PrimitiveKind::Spawn => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "ok".to_string(),
                output: json!({ "spawned": true }),
                artifacts: vec![],
            }),
            PrimitiveKind::Complete => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "completed".to_string(),
                output: json!({}),
                artifacts: vec![],
            }),
            PrimitiveKind::Fail => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "failed".to_string(),
                output: json!({}),
                artifacts: vec![],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_abi::primitives::{PrimitiveId, PrimitiveKind, PrimitiveSpec};
    use serde_json::json;

    fn spec_for(kind: PrimitiveKind, payload: serde_json::Value) -> PrimitiveSpec {
        PrimitiveSpec {
            id: PrimitiveId("test-step".to_string()),
            kind,
            payload,
        }
    }

    #[test]
    fn read_repository_returns_task_payload() {
        let spec = spec_for(PrimitiveKind::Read, json!({"path": "repository"}));
        let result = PrimitiveExecutor::execute("t1", &spec, "hello world").unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.output["content"], "hello world");
        assert_eq!(result.output["content_len"], 11);
    }

    #[test]
    fn executor_prompt_for_answer_question_uses_qa_prompt() {
        let prompt = executor_prompt_for("AnswerQuestion", "What is 17 × 19?");
        assert!(prompt.contains("QUESTION: What is 17 × 19?"));
        assert!(prompt.contains("direct question-answering assistant"));
        assert!(
            !prompt.contains("You are a code executor"),
            "questions must not be framed as code execution"
        );
    }

    #[test]
    fn executor_prompt_for_other_steps_keeps_executor_frame() {
        let prompt = executor_prompt_for("ExecuteChanges", "apply the plan");
        assert!(prompt.contains("Step: ExecuteChanges"));
        assert!(prompt.contains("Detail: apply the plan"));
        assert!(prompt.contains("You are a code executor"));
    }

    #[test]
    fn patch_prompt_substitutes_target_content_and_task() {
        let prompt = patch_prompt("src/calc.rs", "fn add(a:i32,b:i32){a+b}", "fix the add bug");
        assert!(prompt.contains("FILE: src/calc.rs"));
        assert!(prompt.contains("fn add(a:i32,b:i32){a+b}"));
        assert!(prompt.contains("TASK: fix the add bug"));
        assert!(
            !prompt.contains("{target}")
                && !prompt.contains("{content}")
                && !prompt.contains("{task}"),
            "all placeholders must be substituted"
        );
    }

    #[test]
    fn grounded_file_section_reads_real_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(tmp, "marker-content-123").unwrap();
        let path = tmp.path().to_str().unwrap();
        let section = grounded_file_section(&format!("fix the bug in {}", path));
        assert!(section.contains("marker-content-123"));
        assert!(section.contains(path));
    }

    #[test]
    fn grounded_file_section_is_empty_without_target() {
        assert_eq!(grounded_file_section("fix the bug"), "");
    }

    #[test]
    fn write_without_llm_uses_payload_content() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let spec = spec_for(
            PrimitiveKind::Write,
            json!({"path": path, "content": "patch data"}),
        );
        let result = PrimitiveExecutor::execute("t1", &spec, "ignored").unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.output["written_bytes"], 10);
        let written = std::fs::read_to_string(path).unwrap();
        assert_eq!(written, "patch data");
    }

    #[test]
    fn compute_without_llm_returns_computed_status() {
        let spec = spec_for(
            PrimitiveKind::Compute,
            json!({"operation": "none", "detail": "do something"}),
        );
        let result = PrimitiveExecutor::execute("t1", &spec, "payload").unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.output["status"], "computed");
        assert!(result.artifacts.is_empty());
    }

    #[test]
    fn route_without_llm_returns_success() {
        let spec = spec_for(PrimitiveKind::Route, json!({}));
        let result = PrimitiveExecutor::execute("t1", &spec, "payload").unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.output["route_decision"], "Success");
    }

    #[test]
    fn wait_primitive_returns_ok() {
        let spec = spec_for(PrimitiveKind::Wait, json!({}));
        let result = PrimitiveExecutor::execute("t1", &spec, "").unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.output["waited"], true);
    }

    #[test]
    fn signal_primitive_returns_ok() {
        let spec = spec_for(PrimitiveKind::Signal, json!({}));
        let result = PrimitiveExecutor::execute("t1", &spec, "").unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.output["signalled"], true);
    }

    #[test]
    fn spawn_primitive_returns_ok() {
        let spec = spec_for(PrimitiveKind::Spawn, json!({}));
        let result = PrimitiveExecutor::execute("t1", &spec, "").unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.output["spawned"], true);
    }

    #[test]
    fn complete_primitive_returns_completed() {
        let spec = spec_for(PrimitiveKind::Complete, json!({}));
        let result = PrimitiveExecutor::execute("t1", &spec, "").unwrap();
        assert_eq!(result.status, "completed");
    }

    #[test]
    fn fail_primitive_returns_failed() {
        let spec = spec_for(PrimitiveKind::Fail, json!({}));
        let result = PrimitiveExecutor::execute("t1", &spec, "").unwrap();
        assert_eq!(result.status, "failed");
    }
}
