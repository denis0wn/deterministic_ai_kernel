# Public Demo Spec

## Demo purpose
Show a deterministic AI execution engine that produces structured results and can replay the same result without changing outcome.

## Demo audience
Primary audience: AI developers evaluating reliability of AI execution systems.

## Demo message
This system treats AI task execution as a deterministic process with replayable outputs instead of a best-effort black box.

## Demo flow
1. Present one CLI command as the entry action.
2. Show one structured JSON result.
3. Run replay for the same task.
4. Show that the replay result is identical in the same structured fields.

## Demo command
```bash
./target/debug/deterministic-ai-kernel replay-capsule task-replay-json --json
```

## Demo output focus
Highlight only these fields:
- `ok`
- `schema_version`
- `command`
- `report.task_id`
- `report.capsule_id`
- `report.valid`
- `report.events`
- `report.nodes`
- `report.edges`

## Proof point
The public proof is that repeated replay returns the same structured result for the same saved task input.[cite:89][cite:90]

## What to avoid during demo
- internal architecture deep dive
- learning layer discussion
- training / LoRA discussion
- repository setup discussion
- implementation detail overload
