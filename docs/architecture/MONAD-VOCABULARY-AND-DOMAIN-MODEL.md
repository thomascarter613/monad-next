---
id: MONAD-VOCABULARY-AND-DOMAIN-MODEL
title: Monad Vocabulary and Domain Model
status: accepted
version: 0.1.0
date: 2026-05-22
owner: Monad Project
decision_scope: architecture
related:
  - N/A
---

# Monad Vocabulary and Domain Model

## 1. Purpose

This document defines Monad’s canonical product vocabulary.

Monad provides a mature execution-oriented base. Monad extends that base into a governance-grade, AI-ready polyglot repository runtime focused on native-tool orchestration, safe repository evolution, policy, provenance, architecture governance, and repo-native context.

## 2. Status

Accepted.

This document is the canonical reference for naming future user-facing commands, configuration fields, documentation, schemas, runtime state paths, and domain concepts.

## 3. Primary Vocabulary Mapping

| Monad Term | Notes |
|---|---|
| Profile | A named execution/configuration grouping. |
| `profiles/` | Directory containing named profile TOML files. |
| Unit | A repository component such as an app, package, service, library, worker, CLI, or tool. |
| `unit.toml` | Unit-local manifest file. |
| Units | Plural form. Avoid `unites`. |
| Notification | A post-deploy notification task or payload. |
| Notifications | Plural form. |
| Notification payload | Structured payload delivered to notification tasks. |
| `.monad/` | Monad runtime state directory. |
| `.monad/outbox/deploy/` | Replayable deployment notification payload outbox. |
| `monad.toml` | Repository-root Monad manifest. |
| `monad.lock` | Generated resolved state lockfile, if/when implemented. |

## 4. Product Identity

### 4.1 Initial Center of Gravity

Monad's initial center of gravity is:

```text
fast polyglot monorepo orchestration
````

Monad currently focuses on:

* planning tasks
* building
* checking
* testing
* linting
* running
* deploying
* caching
* toolchain coordination
* adapter execution
* CI simplification
* agent-readable JSON output

### 4.2 Planned Center of Gravity

Monad’s planned center of gravity is:

```text
governance-grade, AI-ready polyglot repository runtime
```

Monad includes the abive mentioned execution layer, but its differentiating purpose is broader:

* repository understanding
* safe repository evolution
* supervised plan/diff/apply workflows
* policy enforcement
* provenance recording
* architecture boundary verification
* repo contracts
* ADR/spec/docs-driven workflows
* work-packet-based implementation
* AI-native context generation
* durable repo-native state
* agent handoff and rehydration

## 5. Canonical Monad Terms

## 5.1 Workspace

A **workspace** is the repository root as understood by Monad.

A workspace is the top-level operating boundary for:

* configuration discovery
* profile discovery
* unit discovery
* graph construction
* task planning
* checks
* policy evaluation
* provenance recording
* AI context generation
* repo evolution

Canonical examples:

```text
workspace root
workspace manifest
workspace graph
workspace state
workspace policy
```

Avoid using `project` as the primary term because it is ambiguous in monorepos. A repository may contain many projects, applications, services, packages, and libraries. Monad’s operating boundary is the workspace.

## 5.2 Profile

A **profile** is a named execution/configuration grouping.

A profile answers:

```text
Which units should Monad operate together for this named purpose?
```

A profile may represent:

* `all`
* `release`
* `backend`
* `frontend`
* `staging`
* `production`
* `oss`
* `enterprise`
* `nightly`
* `customer-a`
* `customer-b`

Canonical path:

```text
profiles/<name>.toml
```

Example:

```toml
# profiles/release.toml
name = "release"
units = [
  "apps/api",
  "apps/web",
  "services/worker",
]
```

A unit may belong to multiple profiles.

For example, `apps/api` may belong to both:

```text
profiles/backend.toml
profiles/release.toml
```

The profile is not necessarily a deployment target. It is a named operating set. It can be used for build, test, lint, graph, release, deploy, policy, context, and other workflows.

## 5.3 Unit

A **unit** is a repository component that Monad can understand, plan, check, build, test, lint, run, deploy, graph, or evolve.

A unit may be:

* an app
* a package
* a service
* a library
* a worker
* a CLI
* an infrastructure module
* a documentation site
* a generated SDK
* a database/migration package
* a tool package
* a policy bundle
* an AI context package

Canonical path:

```text
<unit>/unit.toml
```

Example:

```toml
# apps/api/unit.toml
name = "api"
language = "go"

outputs = ["bin/api"]

[tasks.build]
run = "go build -o bin/api ./cmd/api"

[tasks.test]
run = "go test ./..."
```

A unit should be named by what it is in the repository, not by which profile uses it.

Good unit names:

```text
api
web
worker
cli
shared-types
governance-engine
policy-pack
```

Avoid names that only describe deployment grouping:

```text
prod
staging
release
backend-bundle
```

Those are profile names, not unit names.

## 5.4 Task

A **task** is an action Monad can perform for a unit.

Canonical task examples:

```text
build
check
test
lint
dev
serve
run
deploy
release
generate
migrate
seed
policy-check
context-pack
```

Tasks may be:

* adapter-provided
* integration-provided
* user-defined
* generated
* plugin-provided
* future policy/provenance/context tasks

Tasks should remain explicit. Monad coordinates native tools; it does not hide them.

Example:

```toml
[tasks.check]
run = "cargo check --workspace"

[tasks.test]
run = "cargo test --workspace"
```

## 5.5 Adapter

An **adapter** teaches Monad how to understand and operate a language, ecosystem, tool, or repository unit type.

Adapters are responsible for ecosystem-specific behavior such as:

* detection
* default task generation
* toolchain identification
* fingerprint files
* diagnostic parsing
* install behavior
* check/build/test/lint conventions

Examples:

```text
rust
go
bun
node-npm
node-pnpm
python
java-maven
java-gradle
php-composer
ruby
docker
terraform
opentofu
```

Adapters should coordinate native tools rather than replace them.

## 5.6 Integration

An **integration** teaches Monad how to connect a unit/task workflow to an external system.

Examples:

```text
railway
vercel
cloudflare_pages
cloudflare_worker
slack
linear
github
pagerduty
```

Integrations may produce tasks such as:

```text
railway:deploy
vercel:preview
slack:notify
linear:notify
```

Integrations differ from adapters:

```text
Adapter: understands a language/ecosystem/tooling domain.
Integration: connects Monad execution to an external service or workflow sink.
```

## 5.7 Notification

A **notification** is a post-deploy or post-event communication task.

Examples:

* Slack deploy message
* Linear issue transition
* GitHub PR comment
* PagerDuty trigger
* custom webhook
* deployment summary script

A notification payload is structured JSON delivered to a notification task.

Canonical schema name:

```text
notification-payload
```

Canonical type names:

```text
NotificationPayload
NotificationPayloadTrigger
NOTIFICATION_PAYLOAD_SCHEMA_VERSION
```

## 5.8 Outbox

The **outbox** is a local state storage pattern for replayable event payloads.

The outbox is a storage mechanism, not the domain concept itself.

Use:

```text
notification
```

for the domain event/hook.

Use:

```text
outbox
```

for where replayable payloads are stored.

Canonical path:

```text
.monad/outbox/deploy/
```

The deploy outbox exists so that a successful deploy can be followed by replayable notification delivery. If a webhook fails, Monad can rerun notification delivery without rerunning the deploy.

## 5.9 Manifest

A **manifest** is a user-authored configuration file that expresses intent.

Canonical root manifest:

```text
monad.toml
```

Future generated state should not be written back into the manifest unless explicitly requested by the user.

The manifest should answer:

```text
What does the user intend this workspace to be?
```

It should not become a dumping ground for transient runtime state.

## 5.10 Lockfile

A **lockfile** is generated resolved state.

Canonical target:

```text
monad.lock
```

The lockfile should answer:

```text
What did Monad resolve from the user-authored manifest, profiles, units, adapters, toolchains, policies, and plugins?
```

A lockfile should be deterministic and reviewable.

## 5.11 Runtime State

Monad runtime state belongs under:

```text
.monad/
```

Examples:

```text
.monad/cache/
.monad/outbox/
.monad/reports/
.monad/provenance/
.monad/context/
.monad/tmp/
```

Runtime state should be designed carefully so users understand what is safe to commit and what should remain local.

By default, `.monad/` should be treated as generated/local unless specific subpaths are intentionally designed for committed state.

## 5.12 Policy

A **policy** is an explicit rule or rule set that constrains or validates repository behavior.

Policy examples:

* architecture boundary rules
* allowed dependency direction
* required docs
* required ADRs
* required verification scripts
* forbidden runtime dependencies
* package naming conventions
* ownership requirements
* generated file protection
* non-destructive operation requirements
* security rules

Policy should be machine-checkable where possible.

## 5.13 Provenance

**Provenance** records what Monad did, why it did it, and what inputs influenced the result.

Provenance should eventually record:

* command invoked
* resolved workspace
* selected profile
* selected units
* selected tasks
* adapter versions
* toolchain versions
* policy versions
* generated file operations
* dry-run/apply decisions
* timestamps
* result status
* relevant hashes

Provenance is central to Monad’s governance-grade identity.

## 5.14 Context

**Context** is AI-readable repository state generated or assembled by Monad.

Context may include:

* project charter
* ADR summaries
* current repo state
* manifests
* profile/unit graph
* open work packets
* verification status
* policy status
* recent provenance
* architectural boundaries
* handoff instructions

The future `monad context` command should produce context packs that help humans and AI agents understand and continue work safely.

## 6. Directory Model

The target Monad runtime/configuration shape is:

```text
monad.toml
monad.lock
profiles/
  <name>.toml
<unit>/
  unit.toml
.monad/
  cache/
  outbox/
    deploy/
  reports/
  provenance/
  context/
```

Example:

```text
monad.toml
profiles/
  all.toml
  backend.toml
  release.toml
apps/
  api/
    unit.toml
  web/
    unit.toml
services/
  worker/
    unit.toml
.monad/
  outbox/
    deploy/
      release/
        api/
          railway-deploy.json
```

## 7. Command Vocabulary

Monad commands should use serious, general-purpose repository runtime language.

Preferred command names:

```text
monad init
monad info
monad doctor
monad check
monad run
monad graph
monad plan
monad apply
monad diff
monad sync
monad add
monad upgrade
monad context
monad policy
monad audit
monad cache
monad toolchain
monad adapter
monad schema
monad notify
monad deploy
monad release
```

## 8. Schema Vocabulary

Schema names should follow Monad vocabulary.

Preferred schema names:

```text
plan
report
doctor
manifest
diagnostics
notification-payload
profile
unit
workspace
policy-report
provenance-record
context-pack
```

## 9. Public Documentation Rules

Public Monad documentation should use:

```text
workspace
profile
unit
task
adapter
integration
notification
outbox
manifest
lockfile
runtime state
policy
provenance
context
```

## 10. Domain Relationships

The core domain relationships are:

```text
Workspace
  has many Profiles
  has many Units
  has Manifest
  may have Lockfile
  may have Runtime State

Profile
  references many Units

Unit
  has many Tasks
  is claimed by one Adapter
  may have many Integrations

Task
  belongs to one Unit
  may be adapter-provided
  may be integration-provided
  may be user-defined
  may emit reports, diagnostics, artifacts, or events

Integration
  may emit deploy tasks
  may emit notification tasks
  may require environment variables
  may require CLI tools

Notification
  is triggered by an event/task outcome
  receives a NotificationPayload
  may be replayed from the Outbox

Policy
  evaluates Workspace, Profiles, Units, Tasks, files, docs, or provenance

Provenance
  records resolved inputs, actions, decisions, and outputs

Context
  summarizes repository state for humans and AI agents
```

## 11. Canonical Examples

### 11.1 Profile Example

```toml
# profiles/backend.toml
name = "backend"

units = [
  "apps/api",
  "services/worker",
  "packages/shared-types",
]
```

### 11.2 Unit Example

```toml
# apps/api/unit.toml
name = "api"
language = "go"

outputs = ["bin/api"]

depends_on = [
  "shared-types",
]

[tasks.build]
run = "go build -o bin/api ./cmd/api"

[tasks.check]
run = "go vet ./..."

[tasks.test]
run = "go test ./..."
```

### 11.3 Notification Payload Example

```json
{
  "schema_version": 1,
  "monad_version": "0.1.0",
  "environment": "staging",
  "trigger": {
    "task_name": "railway:deploy",
    "unit_name": "api",
    "profile_name": "release",
    "outcome": "built",
    "exit_code": 0,
    "duration_ms": 4272,
    "cache_key": "12dfe62c9f4c",
    "integration_kind": "deploy",
    "output_excerpt": "Build Logs: https://railway.com/...",
    "stderr_excerpt": null
  }
}
```

## 12. Open Questions

The following questions are intentionally left open for future ADRs:

1. Should `profiles/<name>.toml` be required, or can `monad.toml` define inline profiles?
2. Should `unit.toml` be required for every unit, or can units be inferred completely?
3. Should `monad.lock` be committed by default?
4. Which `.monad/` subdirectories, if any, should be commit-safe?
5. How should Monad version schemas over time?
6. How should Monad distinguish local runtime state from team-shared generated state?
7. What is the precise policy engine model?
8. What is the precise provenance event schema?
9. What is the precise context-pack schema?

## 13. Non-Goals

This document does not define:

* final file formats
* final schema structures
* final command behavior
* backward compatibility guarantees
* deprecation timelines
* policy language
* provenance record schema
* context pack schema
* implementation module boundaries

Those require separate ADRs, specs, and implementation slices.

## 14. Decision

Monad adopts the vocabulary in this document as the canonical target vocabulary for future architecture, documentation, CLI behavior, schemas, and code refactoring.

Monad’s vocabulary must support its broader identity:

```text
a governance-grade, AI-ready polyglot repository runtime
```

not merely:

```text
a polyglot monorepo orchestrator