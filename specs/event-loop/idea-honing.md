# Idea Honing

Requirements clarification for the resilient, extensible event loop with hat collections.

---

## Q1: What's the core architectural change you're envisioning?

**Answer:**

The shift is from "Ralph wears different hats" to "Ralph delegates to hat-wearing agents":

**Current design (brittle):**
- Planner and Builder are both "Ralph with a hat"
- Users can override/replace these hats
- This breaks the event graph (events published with no subscriber)
- Ralph can "forget" things

**Proposed design (resilient):**
- Single, irreplaceable "hatless Ralph" — the classic Ralph Wiggum technique
- Hatless Ralph is always present as the orchestrator/manager/scrum master
- Additional hats are optional extensions that Ralph can **delegate to**
- Users ADD hats, they don't REPLACE core Ralph
- Ralph coordinates; hats execute

**Key insight:** Ralph becomes the constant, the orchestrator. Hats become his team.

**Evidence from presets:**
- `review.yml`: `reviewer` triggers on `task.start` — no planner, coordination embedded in reviewer
- `feature.yml`: `planner` is just another replaceable hat
- Each preset rebuilds coordination from scratch
- No safety net for orphaned events

**Root cause:** Coordination is embedded in hats, not separated from them.

---

## Q2: How should hatless Ralph work in practice?

**Answer:**

The existing pub/sub event system stays — hats can still trigger other hats directly (e.g., researcher → reviewer). But hatless Ralph is always **the ruler**.

**Mental model: Constitutional Monarchy**
```
                    ┌─────────────────────────┐
                    │   👑 HATLESS RALPH      │
                    │   (The Ruler)           │
                    │   - Always present      │
                    │   - Ultimate authority  │
                    │   - Oversees everything │
                    └───────────┬─────────────┘
                                │ oversees
        ┌───────────────────────┼───────────────────────┐
        ▼                       ▼                       ▼
   ┌─────────┐             ┌─────────┐            ┌─────────┐
   │ Builder │────event───►│ Reviewer│───event───►│ Deployer│
   └─────────┘             └─────────┘            └─────────┘
        ▲                                              │
        └──────────────────event───────────────────────┘
```

- Hats can still communicate directly via pub/sub
- Users define triggers/publishes as before
- BUT: Ralph is always the sovereign — he rules

---

## Q3: What powers does the ruler have?

**Answer:**

| Power | Has It? | Notes |
|-------|---------|-------|
| **Catches orphaned events** | ✅ Yes | Safety net — no dead ends |
| **Owns completion** | ✅ Yes | Only Ralph can output `LOOP_COMPLETE` |
| **Owns the scratchpad** | ✅ Yes | Ralph creates/maintains; hats read/update |
| **Fallback executor** | ✅ Yes | No hats? Ralph does it himself |
| **Veto power** | ❌ No | Direct hat-to-hat invocation bypasses Ralph |
| **Always runs last** | ✅ Yes | Ralph closes every cycle |

**Key constraints:**
- No veto power — direct hat-to-hat pub/sub bypasses Ralph entirely
- Ralph always runs **last** — he's the closer, not the opener
- Ralph **must** output the completion promise
- Ralph **must** output the final event topic signifying loop complete

**Mental model shift:** Ralph isn't intercepting traffic; he's the final checkpoint.

---

## Q4: When does Ralph run?

**Answer: Option B — When no hat is triggered**

```
hat₁ → hat₂ → hat₃ → (no subscriber for event) → 👑 Ralph runs
```

**Tenet alignment:**
- **Tenet 2 (Backpressure Over Prescription):** Ralph doesn't prescribe when to return; he catches what falls through
- **Tenet 5 (Steer With Signals):** "No subscriber" IS the signal that triggers Ralph
- **Tenet 6 (Let Ralph Ralph):** Hats work autonomously; Ralph only steps in when the chain ends

**Why this is least brittle:**
- Orphaned events don't dead-end — they fall through to Ralph
- No prescription for hats to "hand back" (which they might forget)
- Ralph is the universal fallback, not a micromanager
- The safety net is implicit in the architecture, not explicit in instructions

**Key insight:** Ralph subscribes to `*` (everything), but hat subscriptions take priority. Ralph only activates when no hat claims the event.

---

## Q5: What happens when Ralph runs?

**Answer:**

```
Ralph receives unclaimed event (or no event on first run)
    │
    ├─► "Is there a hat that SHOULD handle this?"
    │       │
    │       ├─► YES: Delegate to that hat
    │       │        (dispatch event that triggers the hat)
    │       │
    │       └─► NO: Handle it myself
    │
    ├─► Update scratchpad with status
    │
    └─► "Is all work complete?"
            │
            ├─► YES: Output LOOP_COMPLETE + final event
            │
            └─► NO: Dispatch next priority task (to hat or self)
```

**Key requirement:** Ralph must know what hats are available and what they do — hat topology must be injected into Ralph's context.

**Two modes:**
1. **Delegate** — There's a hat for this, dispatch to it
2. **Do it himself** — No suitable hat, Ralph handles it directly (classic single-agent mode)

---

## Q6: How does Ralph know what hats are available?

