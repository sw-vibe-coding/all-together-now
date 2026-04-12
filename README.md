# All Together Now

Multi-agent coordination server with shared wiki, event routing, and saga tracking.

## Screenshots

### Agents

4-panel TUI with live terminal sessions for each agent. Coordinator (Claude) is always first.

![Agents view](images/screenshot-agents.png?ts=1776016434792)

### Graph

Dependency graph showing coordinator hub connected to worker agents.

![Graph view](images/screenshot-graph.png?ts=1776016434792)

### Saga

Per-agent saga progress with step-by-step tracking. Coordinator column is always leftmost.

![Saga view](images/screenshot-saga.png?ts=1776016434792)

### Wiki

Shared wiki for inter-agent coordination. Agents read and write pages to exchange links, post requests, and log activity.

![Wiki view](images/screenshot-wiki.png?ts=1776016434792)

### Events

Two-column event log: outbound (Coordinator to Agents) on the left, inbound (Agents to Coordinator) on the right.

![Events view](images/screenshot-events.png?ts=1776016434792)
