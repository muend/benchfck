# Evidence boundary

This directory is reserved for deliberate, release-grade validation artifacts.
Files under `target/` and ad-hoc outputs elsewhere are diagnostics, never evidence.

**Published release artifacts: 0.** The directory is intentionally empty because Phase 2
has not run and its prerequisites remain unmet. `MANIFEST.txt` is policy infrastructure,
not a result artifact.

Every evidence artifact except `MANIFEST.txt` must be listed in `MANIFEST.txt` as:

```text
<lowercase SHA-256><two spaces><path relative to the repository root>
```

Phase 2 evidence must not be generated until gates 2.0a, 2.0b, and 2.0c pass.
