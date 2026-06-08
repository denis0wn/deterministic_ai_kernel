# Execution Contract v1

## Status
Draft

## Version
contract_version: 1

## Purpose
Defines the stable integration boundary between the Swift LocalVoiceAgent client and the Rust deterministic execution kernel.

## Task Envelope
- task_id
- run_id
- task_class
- input
- requested_capabilities
- contract_version

## Event Envelope
- event_id
- run_id
- task_id
- event_type
- ts
- payload
- contract_version

## Canonical Task Classes
- Generic
- CodeFix
- PlannerHardening

## Canonical Step Kinds
- Analyze
- Plan
- ValidatePlannerOutput
- ExecuteChanges
- Verify
- Report

## Terminal Outcomes
- Success
- TerminalFailure

## Non-Terminal Outcomes
- RetryableFailure
- Blocked

## Capability Requirements
- Analyze -> planner
- Plan -> planner
- ValidatePlannerOutput -> verifier
- ExecuteChanges -> executor

## Notes
This file is the initial human-readable contract scaffold. A machine-readable schema should follow in schema/execution_contract_v1.json.

## Event Types
- task.created
- task.started
- task.progress
- task.succeeded
- task.failed
- task.blocked

