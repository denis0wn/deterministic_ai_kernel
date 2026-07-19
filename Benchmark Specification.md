# Benchmark Specification Results

## Experimental Performance Summary

### LLM Direct (Baseline)
- Success Rate: 0.0%
- Mean Wall-Clock Latency: 25.0 ms (SD: 0.0 ms)
- 95% Confidence Interval: [25.0, 25.0] ms
- LLM Call Count: 1.0
- Tool Executions: 1.0

### LLM + Kernel (Cold-Cache)
- Success Rate: 100.0%
- Mean Wall-Clock Latency: 182.0 ms (SD: 4.3 ms)
- 95% Confidence Interval: [179.3, 184.7] ms
- LLM Call Count: 0.0
- Tool Executions: 0.0
- Recovery Events: 1.0

### LLM + Kernel (Warm-Cache)
- Success Rate: 100.0%
- Mean Wall-Clock Latency: 130.3 ms (SD: 3.9 ms)
- 95% Confidence Interval: [127.9, 132.7] ms
- LLM Call Count: 0.0
- Tool Executions: 0.0
- Cache Hits: 3.0
