## Step 7: Scale Demo + Docs

A scripted fleet of 20+ fake agents with varied activity profiles so the
treemap has something to exercise. Plus docs.

### Deliverables

1. `tools/fake-agent-profile` — single script that takes `$ATN_PROFILE`
   (spammer | quiet | periodic | awaiting-input | error) and behaves
   accordingly:
   - spammer: prints a line every 100 ms
   - quiet: prints a banner, then cats stdin
   - periodic: 10 seconds of output, 30 seconds silent, repeat
   - awaiting-input: prints `(y/n) ` and waits
   - error: crashes after 5 seconds with non-zero exit
2. `demos/scale/setup.sh` — POSTs ~20 agents against a running ATN server
   with a mix of profiles: 4 spammers, 8 quiet, 4 periodic, 2 awaiting-
   input, 2 error-after-5s. Names like `spammer-01..04`, etc.
3. `demos/scale/fixtures/` if needed (many small JSON files, or the
   setup script can just template them).
4. `docs/scale-ui.md` — walkthrough: booting the fleet, watching the
   treemap cycle, pinning, keyboard shortcuts, saved layouts. Screenshots
   or ASCII sketches.
5. Update `docs/usage.md` Dashboard section to describe the new layout
   (focus + treemap + compact tiles) and cross-link to `docs/scale-ui.md`.
6. Update `docs/status.md` to note the scale-ui saga.

### Acceptance

- Running `demos/scale/setup.sh` against a fresh empty-start ATN
  populates the dashboard with 20 agents that cycle through states.
- `docs/scale-ui.md` is followable by a fresh reader.
- Screenshots committed to `docs/images/` (or ASCII art if quicker).
