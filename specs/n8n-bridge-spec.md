# n8n Bridge — Spec v0.2

## Overview

Connect Claude Code to n8n so that Claude can programmatically create, manage, and trigger automation workflows. This turns Claude Code into the decision-making brain and n8n into the execution layer — enabling AI-initiated calls, scheduled automations, and event-driven actions.

## Architecture

```
┌─────────────┐       REST API        ┌─────────────────┐
│ Claude Code  │ ───────────────────►  │   Orchestrator   │
│   (brain)    │                       │   Workflow (n8n)  │
└─────────────┘                       └────────┬─────────┘
       │                                       │
       │  reads/writes                         │ triggers
       ▼                                       ▼
┌─────────────┐                       ┌─────────────────┐
│  Registry    │                       │  Module Workflows │
│  (JSON file) │                       │  (n8n)            │
└─────────────┘                       └─────────────────┘
```

### Components

1. **Claude Code** — Reasons about what automations to create, manages the registry, calls n8n API to create/update/trigger workflows.

2. **Orchestrator Workflow** — A single n8n workflow that acts as the entry point. Receives webhook calls from Claude Code, reads the registry, and routes execution to the correct module workflow.

3. **Module Workflows** — Individual n8n workflows, each handling one specific automation (e.g., "notify on server down", "daily standup reminder", "call Dani when deploy fails").

4. **Registry** — A JSON file on the server that maps module names to their n8n workflow IDs, descriptions, and status. All modules are managed by Claude Code.

## Registry

### Location

`/root/projects/trinity-echo/n8n-bridge/registry.json`

### Schema

```json
{
  "version": 1,
  "modules": {
    "<module-name>": {
      "workflow_id": "<n8n workflow ID>",
      "webhook_path": "<module webhook path>",
      "description": "What this module does",
      "triggers": ["webhook", "schedule", "event"],
      "created_at": "ISO 8601",
      "updated_at": "ISO 8601",
      "active": true
    }
  }
}
```

All modules are **managed** — created, maintained, and fully controlled by Claude Code. There are no unmanaged or legacy workflows. Existing workflows will be migrated into the orchestrator as managed modules.

### Migration Status (Phase 2 — COMPLETE)

All standalone workflows migrated to managed modules. Originals deleted.

| Module Name | Workflow ID | Webhook Path | Status |
|---|---|---|---|
| `slack-claude-chat` | `Je439HJU6E6W95N9` | `slack-claude` | Migrated, active |
| `matrix-daily-standup` | `nKwL4c15Er1yZE5N` | `matrix-crew-standup` | Migrated, active |
| `matrix-agent-runs` | `Lp4epPjbUdhZ5ljQ` | `matrix-crew-agent` | Migrated, active |
| Call Workflow | — | — | Was empty shell, deleted |

### Current Registry

```json
{
  "version": 1,
  "modules": {}
}
```

## Orchestrator Workflow

### Design

The orchestrator is a single n8n workflow with a webhook trigger. Claude Code sends POST requests to this webhook with a payload specifying which module to invoke and what data to pass.

### Webhook Endpoint

`POST https://n8n.srv1344383.hstgr.cloud/webhook/orchestrator`

### Payload Schema

```json
{
  "action": "trigger | status | list",
  "module": "<module-name>",
  "data": {}
}
```

### Actions

- **trigger** — Execute a module workflow. Passes `data` to the target workflow.
- **status** — Return the status of a specific module (active, last run, errors).
- **list** — Return all registered modules and their status.

### Orchestrator Flow

1. Receive webhook POST
2. Read registry file from `/home/node/.n8n-files/n8n-bridge/registry.json`
3. Validate auth (X-Bridge-Secret header)
4. Validate payload (action, module)
5. For list/status: return data directly
6. For trigger: call the module's webhook via internal HTTP (`http://127.0.0.1:5678/webhook/<webhook_path>`)
7. Return result to caller

### Implementation Details

- **Workflow ID**: `Pbkkv68BM4IKi09l`
- **Architecture**: Linear — Webhook → Read Registry (file node) → Router (code node)
- **Response mode**: `lastNode` — the Router code node output is returned directly
- **Module execution**: Modules have their own webhook triggers. The orchestrator calls them internally via `this.helpers.httpRequest()` in the code node
- **Registry file**: Mounted from host `/local-files/n8n-bridge/` to container `/home/node/.n8n-files/n8n-bridge/`

## Claude Code Integration

### How Claude Code Interacts

Claude Code uses two channels:

1. **n8n REST API** (direct) — For CRUD operations on workflows:
   - Create new module workflows
   - Activate/deactivate workflows
   - Read workflow definitions
   - Delete managed workflows

2. **Orchestrator Webhook** — For runtime operations:
   - Trigger a module
   - Check module status
   - List available modules

### API Access

- **Base URL**: `http://127.0.0.1:5678/api/v1`
- **API Key**: stored in `/docker/n8n/.env` or referenced from server env
- **Auth Header**: `X-N8N-API-KEY: <key>`

### Creating a New Module (flow)

1. Dani asks Claude Code: "Set up a notification when the server CPU goes above 90%"
2. Claude Code designs the workflow nodes (trigger + logic + action)
3. Claude Code calls `POST /api/v1/workflows` to create the workflow in n8n
4. Claude Code activates the workflow via `POST /api/v1/workflows/{id}/activate`
5. Claude Code registers it in the registry as a managed module
6. Claude Code confirms to Dani

## AI-Initiated Calls

### How It Works

Module workflows can trigger outbound calls through trinity-echo. The flow:

1. n8n module detects an event (schedule, webhook, threshold)
2. Module calls trinity-echo's API or Twilio to initiate an outbound call
3. When Dani picks up, he's connected to Claude Code with full context about why it called

### Context Passing

When an AI-initiated call is triggered, the module passes context to Claude Code so it knows why it's calling. This context includes:
- Which module triggered the call
- The event data (what happened)
- Suggested actions or information to relay

*Details of the outbound call mechanism TBD — depends on trinity-echo's outbound support.*

## File Structure

```
/root/projects/trinity-echo/
├── n8n-bridge/
│   └── registry.json          # Module registry (symlinked via /local-files/)
└── specs/
    └── n8n-bridge-spec.md     # This spec

/local-files/n8n-bridge/
└── registry.json              # Actual registry location (mounted into n8n container)

/docker/n8n/
├── docker-compose.yml         # n8n container config (volume: /local-files → /home/node/.n8n-files)
└── .env                       # n8n environment variables
```

## Security

- n8n API is only accessible via localhost (127.0.0.1:5678), not exposed externally
- Orchestrator webhook is protected with a secret token via `X-Bridge-Secret` header
- **Bridge Secret**: `2686e5a38c51d26a5b9f72642be359291bce1143ec988bc9e205baea6a3d13ff`
- API key is scoped to Claude Code operations only
- Managed workflows created by Claude Code follow naming convention: `[bridge] <module-name>`
- Module webhooks use path prefix `bridge-` (e.g., `bridge-test-ping`)

## Rollout Phases

### Phase 1 — Foundation

- Build the orchestrator workflow in n8n (webhook + router logic)
- Create the empty registry file
- Wire up Claude Code to talk to n8n's REST API (create, activate, trigger)
- Test end-to-end: Claude Code creates a simple test module, orchestrator routes to it, result comes back
- Add webhook security (secret token)

### Phase 2 — Migration

- Rebuild each existing workflow as a managed module under the orchestrator
- Migrate one at a time, validate, then deactivate the standalone original
- Fix the matrix-crew-agent-runs errors during its migration
- Order: slack-claude-chat → matrix-daily-standup → matrix-agent-runs → call-handler (most critical, last)

### Phase 3 — AI-Initiated Calls

- Build outbound call capability (Twilio outbound via n8n or trinity-echo)
- Create a generic "call Dani" module that accepts context and initiates a call
- Other modules can trigger it when events occur
- Claude Code receives context about why it's calling when the call connects

### Phase 4 — Expansion

- Templates for common module patterns (notify, schedule, monitor)
- Dani can request new automations conversationally and Claude Code builds them live
- Self-monitoring: orchestrator health checks, module failure alerts

## Open Questions

1. **Outbound calls**: trinity-echo currently handles inbound. What's the plan for outbound call initiation? Twilio API direct or through trinity-echo?
2. **Error handling**: When a module fails, should Claude Code be notified? Via what channel?
3. **Rate limiting**: Should there be limits on how many workflows Claude Code can create?
4. **Matrix Crew workflows**: The agent-runs workflow has errors in logs. Should it be fixed or deprecated?
5. **Webhook security**: Bearer token, HMAC signature, or something else for the orchestrator webhook?
