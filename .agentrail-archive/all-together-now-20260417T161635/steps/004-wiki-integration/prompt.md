## Phase 4: Wiki Integration

Shared coordination wiki accessible from UI and agents.

### Deliverables

1. Wiki REST endpoints in atn-server (GET, PUT, PATCH, DELETE pages) with CAS via ETag
2. Wiki browser component in Yew UI (view, edit, wiki-links)
3. Markdown rendering with wiki-links (reuse wiki-common parser)
4. Seed coordination pages on startup (Goals, Agents, Requests, Blockers, Log)
5. CAS conflict handling (409 response with current page)

### Acceptance Criteria

- Wiki pages accessible via REST API with proper ETag/CAS
- Wiki browser in UI renders markdown and wiki-links
- Coordination pages exist after first startup
- cargo test --workspace passes
