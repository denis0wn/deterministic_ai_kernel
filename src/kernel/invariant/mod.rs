// invariant layer: deterministic state reconstruction
// documented cross-layer dependency: kernel/invariant -> engine/event_bus
// this is the only allowed upward reference from kernel to engine
pub mod replay;
