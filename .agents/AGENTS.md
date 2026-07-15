# Workspace Customization Rules

## Testing and Verification
- **Rule**: When executing task verification, compiling, or running tests in this repository, you MUST use the `./ct` test runner wrapper (located at the workspace root) instead of direct `cargo test` commands.
- **Reason**: The `./ct` wrapper filters out warnings, compiler noise, and successfully passed tests, outputting only `✅ ALL TESTS PASSED` on success, while exposing errors/failures clearly on failure. This dramatically improves developer experience (DX) and keeps log traces clean.
- **Usage Guidelines**:
  - To check compilation and run all tests: `./ct`
  - To run with specific cargo test options: `./ct [args]` (e.g. `./ct --test scheduler_integration`)
  - The script preserves standard exit codes (0 for success, non-zero for failure).
