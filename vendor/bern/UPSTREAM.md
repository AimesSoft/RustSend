# Bern source snapshot

- Upstream: https://github.com/AimesSoft/Bern
- Baseline: `ffbdbdf3d41bb6492a8b13d3fa6182ee636606ee`
- License: [MIT](LICENSE)
- Snapshot date: 2026-10-23

This snapshot fixes the exact framework sources used by RustSend 1.0.0, including local platform integration, storage, monochrome controls, and iOS device-build changes. It is not an unmodified upstream release. The root workspace omits the upstream examples; framework crates and their license are preserved. Application builds use this copy, never a sibling checkout or a moving branch.

The RustSend release commit identifies the complete snapshot; update it explicitly when upgrading Bern.

## Changes since 1.0.0

- 2026-10-31: Bern's `spin` and `spin_period` motion properties, ported from the Bern working tree after `3085076` (not yet an upstream commit): the IR and its checks, the target capability check, the SwiftUI, Compose, Web, WinUI 3 and GTK backends and runtimes, and their tests, except the GTK runtime's test, which needs a context constructor this snapshot lacks. The rest of the snapshot is unchanged; a full upgrade must carry its local changes over, since 64 of the 89 locally changed files conflict with current Bern.
