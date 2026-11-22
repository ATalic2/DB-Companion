# DbCompanion

A desktop database client with an AI agent built in — connect to a database, chat with an AI about what you want to do, review the plan it proposes, and choose when to apply it.

Built with [Tauri](https://tauri.app) (Rust backend) + React/TypeScript (Vite) frontend as a hands-on exploration of AI agent design — prompt engineering, structured plan/execute workflows, and safety controls around letting an LLM propose changes to a live database — paired with practical work across multiple database engines.

## Features

- **Multi-database support:** PostgreSQL, MySQL, SQL Server (MSSQL), MongoDB, SQLite — all behind a common adapter interface.
- **AI chat agent:** ask questions about your schema, or describe a change in plain English. Each request includes the connected database's schema (tables, columns, indexes, constraints) and recent chat history, so the AI can answer directly or propose a structured **change plan**.
- **Plan → Review → Execute flow:** proposed commands are shown with a risk score before they run. Apply executes the displayed plan. Transaction and rollback behavior depends on the database engine and the commands involved.
- **Query editor:** SQL editor for relational databases, a JSON/Script-mode editor for MongoDB (including running full multi-statement `mongosh` setup scripts).
- **Schema browser:** live view of tables/collections, columns, indexes (with real per-engine index types — B-tree, hash, fulltext, spatial, columnstore, text, geo, etc.), and constraints.
- **Credential vault:** database passwords, connection strings, and AI provider keys are encrypted at rest. Windows protects new secrets with current-user DPAPI; macOS and Linux use AES-256-GCM. Profile details such as labels and hosts are stored locally as app settings.

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

On first launch, use the **Manage** links beside Database and AI Provider Key to add a database connection and a Gemini or OpenAI API key.

## Releasing

Push a semantic version tag to build and publish that version on every platform:

```bash
git tag v0.4.0
git push origin v0.4.0
```

The release workflow derives `0.4.0` from the tag, synchronizes the package and
Cargo metadata inside the CI runner, and passes a generated version override to
Tauri. These generated changes are used for the release build and are not
committed back to the repository. Manual workflow runs accept the version as an
input. The About screen reads the packaged Tauri version at runtime.

## License

MIT — see [LICENSE](./LICENSE).
