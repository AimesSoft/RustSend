# Bern source snapshot

- Upstream: https://github.com/AimesSoft/Bern
- Baseline: `ffbdbdf3d41bb6492a8b13d3fa6182ee636606ee`
- License: [MIT](LICENSE)
- Snapshot date: 2026-10-23

This snapshot fixes the exact framework sources used by RustSend 1.0.0, including local platform integration, storage, monochrome controls, and iOS device-build changes. It is not an unmodified upstream release. The root workspace omits the upstream examples; framework crates and their license are preserved. Application builds use this copy, never a sibling checkout or a moving branch.

The RustSend release commit identifies the complete snapshot; update it explicitly when upgrading Bern.
