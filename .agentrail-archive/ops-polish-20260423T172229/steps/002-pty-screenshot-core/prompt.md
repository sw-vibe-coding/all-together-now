## Step 2: vte-driven terminal snapshot core

Add a shared snapshot helper that turns the last-N transcript bytes
into a rendered terminal grid (text, ANSI, or HTML). atn-replay
already has vt100-based rendering for full-session replay; this
shares that implementation so both the CLI and the HTTP endpoint
produce identical output.

### Deliverables

1. New module — either `atn-pty::snapshot` or a thin `atn-snapshot`
   helper crate — exposing:
   ```rust
   pub struct TerminalSnapshot { pub rows: usize, pub cols: usize,
                                  pub cells: Vec<CellRow> }
   pub fn snapshot_from_bytes(bytes: &[u8], rows: usize, cols: usize)
       -> TerminalSnapshot;
   pub fn render_text(&self) -> String;
   pub fn render_ansi(&self) -> String;   // preserves fg/bg/bold
   pub fn render_html(&self) -> String;   // minimal CSS, self-contained
   ```
2. Factor atn-replay's rendering over the same core if they diverge,
   so there's one truth for "what does the terminal look like right
   now".
3. Unit tests: simple escape sequences (cursor move, color, clear
   screen) all produce the expected grid.
4. Benchmarks live in criterion only if needed; simple asserts are
   fine for the unit tests here.

### Acceptance

- `cargo test -p atn-pty` (or whichever crate owns the module) passes
  snapshot round-trip tests.
- atn-replay still renders transcripts correctly (no regression).
- cargo doc warning-free.