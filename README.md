# DbCompanion

A desktop database client with an AI agent built in — connect to a database, chat with an AI about what you want to do, review the plan it proposes, and execute it (or dry-run it first).

Built with [Tauri](https://tauri.app) (Rust backend) + React/TypeScript (Vite) frontend as a hands-on exploration of AI agent design — prompt engineering, structured plan/execute workflows, and safety controls around letting an LLM propose changes to a live database — paired with practical work across multiple database engines.

## Features

- **Multi-database support:** PostgreSQL, MySQL, SQL Server (MSSQL), MongoDB, SQLite — all behind a common adapter interface.
- **AI chat agent:** ask questions about your schema, or describe a change in plain English. The AI reads your live schema (tables, columns, indexes, constraints) and either answers directly or proposes a structured **change plan**.
- **Plan → Review → Execute flow:** every proposed change is shown as a plan with a risk score before anything runs. Dry-run mode lets you preview a change without committing it; execution is transaction-wrapped where the engine supports it, with automatic rollback on failure.
- **Query editor:** SQL editor for relational databases, a JSON/Script-mode editor for MongoDB (including running full multi-statement `mongosh` setup scripts).
- **Schema browser:** live view of tables/collections, columns, indexes (with real per-engine index types — B-tree, hash, fulltext, spatial, columnstore, text, geo, etc.), and constraints.
- **Credential vault:** connection details and AI provider keys are encrypted at rest (AES-256-GCM) rather than stored in plaintext.

## Getting started

### Prerequisites

- [Node.js](https://nodejs.org) (v18+)
- [Rust toolchain](https://www.rust-lang.org/tools/install) (stable)
- Platform build dependencies for Tauri — see the [Tauri prerequisites guide](https://tauri.app/start/prerequisites/) for your OS
- [`mongosh`](https://www.mongodb.com/try/download/shell) installed and on your `PATH`, if you plan to connect to MongoDB

### Setup

```bash
npm install
npm run tauri:dev      # run in development
npm run tauri:build    # build a release binary
```

On first launch, add a database connection and an AI provider key (currently Gemini) from **Settings**.

## License

MIT — see [LICENSE](./LICENSE).
