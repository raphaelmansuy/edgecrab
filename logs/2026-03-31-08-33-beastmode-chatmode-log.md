Task logs

Actions: Ran strict clippy and targeted skills tests; fixed all clippy errors in skills.rs; refactored duplicated GitHub auth/header logic into apply_github_auth; added deterministic bundle hash persistence in skills_hub install flow; replaced flaky lock-based test with deterministic hash stability test.
Decisions: Treated clippy -D warnings as quality gate; prioritized behavior-preserving refactors for DRY/SOLID; kept update lifecycle as baseline status+reinstall while ensuring content_hash is now populated.
Next steps: Review and commit current modified files, then optionally wire available_tools into ToolContext to remove remaining conditional-activation TODO.
Lessons/insights: Biggest correctness gap was empty content_hash in lock entries; strict linting quickly exposed structural cleanups needed for maintainability.
