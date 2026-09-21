# Stateful Codex product intent

This document preserves the user problem and intended experience behind
Stateful Codex. Read it before evaluating the architecture, implementation, or
UI. The product charter and engine specification define technical contracts;
this document defines what those contracts are meant to accomplish for people.

## The problem

Current agentic systems break down on long-running, complex projects.

1. Context becomes expensive as the project grows.
2. Compaction loses important details and may shift attention to the wrong
   files, facts, or assumptions.
3. Complex knowledge work rarely begins with a complete plan. The useful
   strategy has to emerge from the material as it is examined.
4. Exceptional work often depends on finding one decisive number, sentence,
   clause, contradiction, or connection hidden in a large corpus.
5. Existing interfaces expose tool calls and low-value activity, then produce a
   large final result that the user can only audit after the work is done.
6. The user therefore has to carry project context, monitor execution, and
   reconstruct whether the result is trustworthy.

Stateful Codex should let a person delegate substantial project work without
repeatedly rebuilding context, supervising routine execution, or waiting until
the end to understand what the system learned.

## Intended user experience

The user opens a project directory, states the desired outcome, chooses how the
system should work, and lets it proceed.

- The selected directory and its subdirectories define the project. The system
  does not infer which project a request belongs to.
- The user chooses whether to continue, create, or fork a thread. Threads are
  conversation and execution views, not project-memory boundaries.
- The user chooses Autonomous, Collaborative, or Socratic mode. The system does
  not infer the mode.
- Autonomous mode should keep working without unnecessary checkpoints. The user
  may leave it unattended or intervene when their judgment is useful.
- Important project understanding survives compaction and thread changes.
- The system explains meaningful learning, implications, uncertainty, and
  changes of strategy while the work is happening.
- The final result remains traceable to evidence and clearly states what was
  accomplished, what remains uncertain, and whether the result is usable.

The intended product is not chat with memory. It is a continuously improving
project intelligence and execution environment.

## Hierarchical blackboards

The blackboard is the structured, persistent memory of what the system has
learned. Its purpose is to let future work begin from accumulated understanding
instead of repeatedly rereading the project.

The hierarchy mirrors the filesystem:

```text
Project
└── Directory
    └── Subdirectory
        └── File
            └── Optional section or atomic unit
```

The optional final level is a generic anchored region within a file. A function,
clause, table, chapter, or passage is the same core abstraction: a bounded unit
with an identity or locator. The core should not impose separate domain-specific
hierarchies such as document/article/clause or module/type/function.

### Root blackboard

The root blackboard is always available to the model. It may be relatively rich
because the system is designed for long-context models. It should include the
project-wide material most likely to improve work across tasks:

- important user instructions and constraints;
- central facts and strategic conclusions;
- high-value findings and decisions;
- cross-source relationships and contradictions;
- open questions and unresolved signals;
- important failures and rejected approaches; and
- miscellaneous information whose broader relevance may not yet be known.

The root is intentionally broad. Its cross-project awareness allows the model to
notice a connection between facts that would appear unrelated if it saw only the
currently selected file. Finding those non-obvious but valid connections is one
of the product's highest-value outcomes.

### Deeper blackboards

Directory and file blackboards preserve more specific understanding. Optional
section-level records provide precision without requiring the entire file or
blackboard to enter context.

The state should be structured and queryable: facts, claims, numbers,
instructions, decisions, strategies, questions, contradictions, evidence,
provenance, and relationships should be retrievable without loading the whole
blackboard.

Maintenance should progressively enrich this state, connect information across
files, expose contradictions, identify unresolved questions, and surface
strategic possibilities that are not visible from any single source.

## Context map

The context map is separate from the blackboard.

- The blackboard says what the system understands.
- The context map says where information can be found.
- The source files remain the ground truth.

The context map is the index to the project corpus. It records what files and
bounded regions contain, so the model can move from a question to the likely
source without searching or rereading everything.

The model uses the context map when:

- the blackboard lacks sufficient detail;
- a potentially important detail may have been omitted;
- existing state needs enrichment or repair;
- a consequential claim requires verification against the original source; or
- related material may exist elsewhere in the project.

The desired access pattern is:

```text
Always-loaded root blackboard
        ↓
Relevant directory/file blackboard
        ↓
Context-map lookup
        ↓
Exact source region when needed
```

The long-term goal is for mature blackboards to become the primary working
representation. Raw files should be read selectively rather than repeatedly.
They remain necessary for exact verification and for form-sensitive work such
as copying style, formatting, layout, or structure. In those cases the relevant
source may need to be pinned directly into model context.

## Obligation packet and real-time transparency

The obligation packet is a compact, evolving semantic account of the work. It
is not a tool-call log and not merely a completion checklist. Roughly 500 useful
words may be appropriate; the goal is information value, not arbitrary brevity.

It should explain:

- what was examined;
- why it mattered;
- what was learned;
- the implication of that learning;
- the current strategy and obligations;
- what changed and why;
- what the system will explore or do next;
- material uncertainty or blockers; and
- where user knowledge or judgment could improve the work.

It should not narrate activity such as "reading this file" or "searching these
lines" without explaining the result and significance.

Updates should correspond to meaningful discoveries, changed strategies,
important uncertainty, blockers, or usable results. Routine tool activity
should remain grouped beneath the work it serves.

This makes real-time steering valuable. The user can say:

- this fact is important; follow it;
- look for similar evidence elsewhere;
- connect this with another finding;
- add this consideration;
- this direction is not useful; or
- this interpretation is wrong.

The user becomes an informed collaborator during the investigation instead of
an auditor waiting for a large opaque result.

Transparency must not make the system timid. User-defined goals and binding
constraints cannot be silently rewritten, but system-derived obligations,
hypotheses, and strategies should evolve visibly as evidence changes.

## Long-running product goal

The expected progression is:

```text
Early workspace:
frequent raw-source reading

Maturing workspace:
blackboard-first work with targeted source verification

Rich workspace:
most reasoning from accumulated structured state,
with raw sources opened only when the task requires them
```

Exact root contents, promotion rules, packet size, and loading volume are
iteration variables. Long-context models allow the root and miscellaneous
high-value state to be richer than a minimal-memory design. These choices should
be evaluated empirically rather than prematurely constrained.

## Review lens

When the detailed implementation review is requested, do not judge the system
only by the existence of databases, state machines, APIs, packets, or UI panels.
Ask whether the implementation actually:

- reduces repeated reading and lifetime token cost;
- preserves important information through compaction and thread changes;
- maintains useful structured understanding at the correct project level;
- routes efficiently from blackboard state to exact source material;
- discovers decisive details and non-obvious cross-source connections;
- supports strategies that emerge and change as evidence arrives;
- produces concise, meaningful obligation updates;
- enables high-value real-time user steering;
- remains productive when left to work autonomously; and
- returns evidence-grounded results that the user can understand and use.

That is the standard the architecture and implementation must ultimately meet.
