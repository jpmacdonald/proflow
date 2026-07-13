# Agent Engineering Rules

This repository is maintained under an invariant-first workflow. These rules are
the default review standard for handwritten code.

## Rule Propagation

- When a project-specific `AGENTS.md` or `CLAUDE.md` contains a broadly useful
  maintainability rule, promote it into this guide or copy it into sibling
  project guides during adoption.
- Keep project-specific nouns local. Promote the boundary principle, ownership
  rule, or verification standard behind the rule.
- Local exceptions must name the reason and scope. Do not silently weaken shared
  safety, invariant, or maintainability rules.

## Priority Order

```text
safety
semantic correctness
test strength
performance
developer experience
```

Developer experience matters because it helps humans and agents preserve the
first four priorities. It does not outrank them.

## Core Rules

1. Make invalid states unrepresentable before adding runtime checks.
2. Prefer enums, newtypes, private fields, and checked constructors over strings,
   booleans, nullable fields, and comments that callers must remember.
3. No public API may require a caller to perform a hidden earlier step. Encode
   the state transition in the type or return a typed error.
4. Do not use `unwrap`, `expect`, or `panic` in production code unless the
   project has an explicit local exception.
5. Do not add fallback behavior for semantically invalid states. Fail loudly with
   typed errors.
6. Golden and snapshot changes must be intentional and reviewed as behavior
   changes.
7. Any bug fix involving domain behavior must add a regression test.

## Maintainability Doctrine

This project optimizes for reader cost.

A reader should be able to understand a change without spelunking through
needless indirection or simulating hidden state in their head.

Two axioms govern all code:

1. Reduce trace depth.
2. Reduce live state.

These axioms outrank cleverness, premature abstraction, aesthetic symmetry,
DRY-for-DRY's-sake, framework idioms, and local terseness. If a change increases
trace depth or live state, it must pay for itself clearly in correctness,
semantic precision, or product value.

### Reduce Trace Depth

Trace depth is the number of files, functions, types, callbacks, traits,
interfaces, wrappers, configs, generated artifacts, or framework conventions a
reader must follow to understand what actually happens.

Prefer code that makes control flow locally obvious.

Do:

- Keep behavior close to the call site when the behavior is not reused
  meaningfully.
- Inline tiny abstractions that only rename one thing.
- Prefer straightforward functions over class hierarchies, registries,
  factories, middleware stacks, callback chains, or single-implementation traits.
- Make dependencies explicit in function parameters or struct fields.
- Keep module boundaries meaningful: a module should hide real complexity, not
  scatter it.
- Use abstraction only when it removes more tracing than it adds.

Do not:

- Add a layer merely because it might be useful later.
- Hide simple logic behind interfaces, adapters, helpers, managers, services, or
  context objects.
- Split one concept across many files unless the split reduces total reader
  burden.
- Create generic mechanisms for one or two concrete cases.

Before adding an abstraction, answer:

1. What concrete repetition or complexity does this remove?
2. How many places must a reader now inspect to understand behavior?
3. Does this reduce total trace depth, or merely move code elsewhere?

If the abstraction does not clearly reduce trace depth, do not add it.

### Reduce Live State

Live state is the set of facts a reader must keep in mind to understand
correctness at a given point in the code.

This includes mutable variables, object fields, global state, environment state,
caches, implicit context, initialization order, temporal coupling, side effects,
ownership/lifetime assumptions, and invariants spread across functions.

Prefer code where data flow is explicit, narrow, and stable.

Do:

- Prefer immutable values and small scopes.
- Prefer pure functions when practical.
- Pass required data explicitly.
- Return new values instead of mutating shared ones when reasonable.
- Keep state transitions obvious and localized.
- Name state by what it means, not by where it came from.
- Collapse related state into a single well-named type when that reduces mental
  load.
- Document invariants at the type or function boundary where they matter.

Do not:

- Use globals, singletons, hidden context, ambient config, or thread-local state
  unless unavoidable.
- Mutate state from distant code.
- Require callers to remember ordering constraints.
- Let partially initialized objects escape.
- Encode state implicitly through booleans, nullable fields, magic strings, or
  side-effect timing.
- Make readers infer behavior from setup that happened far away.

Before introducing mutable or implicit state, answer:

1. Who owns this state?
2. Who can change it?
3. When can it change?
4. What invariant must remain true?
5. Can the same result be expressed with less state?

If the answers are not obvious from the code, redesign.

## Cleanup Laws

- One owner per state.
- One translation per boundary.
- No shadow runtime state in host layers.
- Do not restate the same concept across multiple intermediate layers.
- Keep one canonical Rust contract per boundary and one consumer contract per
  language boundary.
- Do not add crates or framework layers for cleanup alone.
- Generated code is out of scope; clean the wrappers and ownership boundaries
  around generated code.
- Delete dead compatibility paths immediately unless the project explicitly
  requires legacy support.

## Compiler And Workflow Boundaries

When the project has compiler-like phases, keep them separate:

```text
parse -> normalize -> plan -> execute/render -> validate/export
```

Rules:

- Each phase takes input and returns output. No hidden globals.
- No IO in middle phases unless the project explicitly defines that phase as an
  IO boundary.
- Prefer rebuild over mutation: `fn plan(...) -> Plan`, not
  `fn update_plan(&mut Plan)`.
- Keep enums tight: one variant per semantic concept, not mega variants that do
  everything.
- Backend, transport, UI, and storage details must not leak upward into semantic
  models.
- Capability matrices and typed policies should replace scattered platform or
  environment branches.
- Validation should derive from the same predicates or postconditions as the
  operation it validates.
- Ambiguity is a typed error or explicit manual classification, not a best-effort
  guess.

## Unsafe Boundaries

The default policy is no `unsafe`. A project may allow `unsafe` only for a named
reason such as SIMD, FFI, JIT execution, or a measured hot path that cannot be
expressed safely.

When `unsafe` is allowed:

- Keep every unsafe boundary small and locally auditable.
- State the safety invariant near the unsafe block or function.
- Validate pointers, lengths, alignment, aliasing, and lifetimes before crossing
  the boundary.
- Provide safe wrappers so the rest of the codebase cannot misuse the unsafe
  primitive.
- Cover the boundary with tests, property tests, fuzz tests, or corpus tests.
- Do not let unsafe APIs leak into semantic/domain layers.

## Structural Targets

Project-specific targets may override these, but the default shape is:

- Coordinator/runtime modules: `<= 500` lines.
- App/CLI command or boundary modules: `<= 350` lines.
- Entry/facade files such as `lib.rs` and `main.rs`: `<= 250` lines.
- Test/harness files: `<= 800` lines unless fixture-heavy and intentionally
  isolated.

These are review triggers, not mechanical excuses to split one concept across
more files.

## Red Flags

Be suspicious of vague names such as:

- `Manager`
- `Handler`
- `Processor`
- `Service`
- `Adapter`
- `Helper`
- `Utils`
- `Factory`
- `Context`
- `Registry`
- `Engine`
- `Orchestrator`

These names are not forbidden, but they often indicate vague responsibility or
unnecessary layering. Choose a more specific name when one exists. If no
specific name exists, the abstraction is probably not real yet.

## Required Verification

Use the repository's `just` interface:

```sh
just local
just ci
just deep
```

Rules:

- Do not present ad hoc verification as sufficient when an equivalent `just`
  target exists.
- During debugging, narrow to the smallest failing command.
- Before claiming completion, rerun the enclosing `just` target.
- For invariant-heavy changes, `just deep` is the default completion gate.
- If a required tool is unavailable, report the exact missing tool and the
  highest gate that did pass.
- In every completion summary, report exact failing commands, exact passing
  commands, and exact commands not run.

## Review Questions

Before keeping a design, answer:

1. What invalid state became unrepresentable?
2. Which invariant is enforced by the compiler, not by prose?
3. Which invariant is enforced by property, fuzz, snapshot, model, or mutation
   tests?
4. Where is the single owner of each state touched by this change?
5. How many files must a reader inspect to understand the state transition?
6. Is any boundary contract restated unnecessarily?
7. Did this add a wrapper layer that only renames existing behavior?
8. Can an agent make the wrong call sequence and still compile?
9. Can the operator-visible outcome be explained in at most three files?
