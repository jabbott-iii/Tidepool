---
name: evaluate-dependency
description: Use before proposing any third-party crate or vendored code. Runs the build-vs-buy test from docs/02-dependency-policy.md and writes an evaluation for the user to decide on. Never adds the dependency.
argument-hint: "<crate-name> <what it would be used for>"
---

# Evaluate a proposed dependency

Proposal: `$ARGUMENTS`

The project rule: **build it ourselves unless building it is unreasonable.**
You are writing an evaluation for the user to decide on. You never approve
it yourself, and you never change any `Cargo.toml` or `Cargo.lock`.

## Steps

1. **Restate the need.** Inputs, outputs, performance, platforms, and where
   it would live (crate and layer).
2. **Try to remove the need.** List design changes, each with its cost:
   - a simpler format or subset
   - reduced scope
   - a **managed tool** doing the work as a child process (ADR-0002),
     the way HTTPS goes through `curl` and signing through `apksigner`
   - a platform API we already bind
3. **Estimate building it ourselves:** components and approximate lines,
   specs to implement (with links), test strategy (vectors, property tests,
   fuzzing), risks, engineer-weeks as a labeled range. Check whether the
   owner's Nadir repository already has code to port.
4. **Apply the exception test**, criterion by criterion, "met" or "not met"
   with a reason:
   1. Disproportionate cost: over 4 engineer-weeks, not central, no design
      change or managed tool avoids it.
   2. Specialist correctness with severe failure modes.
   3. Dev-only tooling with no reasonable equivalent, never shipped.
5. **Gather facts about the candidate** with the web tools: license, latest
   version and date, maintainers and activity, transitive dependency count,
   `unsafe` usage, MSRV, binary-size impact, whether it ships in the
   Tidepool binary. **Never add the crate to this workspace to inspect it.**
   Write "unverified" for anything you can't check.
6. **Write the evaluation** to `docs/dependency-evaluations/<crate>.md`
   using the template below.
7. **Ask the user for a decision.** If approved, follow the policy's
   "Exception process" step 3 as a separate change (register row, ADR via
   `/record-decision`, allowlist line, exact pin, one wrapping module). If
   rejected, record that in the evaluation and the register.

## Template

```markdown
# Dependency evaluation: <crate> (<version>)

- **Date:** YYYY-MM-DD
- **Requested for:** <capability>, used in <crate/layer>
- **Ships in the Tidepool binary:** yes | no (dev/tool only)

## Need
<concrete description>

## Design changes or managed tools that would avoid it
| Option | Cost | Viable? |
| :- | :- | :- |

## Build-it-ourselves estimate
<components, specs, test strategy, risks, estimate range, Nadir code to port>

## Exception test
| Criterion | Met? | Reason |
| :- | :- | :- |
| 1. Disproportionate cost | | |
| 2. Specialist correctness | | |
| 3. Dev-only, no equivalent | | |

## Candidate facts
| Fact | Value | Verified? |
| :- | :- | :- |
| License | | |
| Latest version / date | | |
| Transitive deps | | |
| `unsafe` usage | | |
| MSRV | | |
| Binary size impact | | |

## Recommendation
<build | adopt with conditions | reject>, with one paragraph of reasoning.

## Decision
<left blank for the user>
```
