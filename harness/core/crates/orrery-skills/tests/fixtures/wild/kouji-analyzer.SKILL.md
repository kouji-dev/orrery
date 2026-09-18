---
name: kouji:analyzer
description: Use when the user wants to analyze their LinkedIn profile (strengths, weaknesses, market positioning), follow up on open job application statuses, or get a stats summary of their job hunt. Trigger on "analyze my profile", "profile review", "pros and cons", "follow up on jobs", "any updates", "how is my job hunt going", "job hunt stats".
---

# Kouji Analyzer

Three modes: **profile** analysis, **followup** on open applications, **stats** summary.

## Mode routing

| User says | Mode |
|---|---|
| "analyze my profile", "profile review", "pros and cons", "how to position myself" | profile |
| "follow up", "any updates", "check my applications", "review open jobs" | followup |
| "stats", "summary", "how is it going", "job hunt progress" | stats |

## Mode: profile

**Goal:** PROS/CONS + market positioning from `context.md`, optionally enriched by live LinkedIn profile.

1. Read `context.md`. If missing, suggest `kouji:hunter context rebuild` first.
2. Optionally navigate to user's LinkedIn profile via `mcp__claude-in-chrome__navigate`. Ask for profile URL if not in `criteria.yaml#linkedin_url`.
3. Output — four sections:

   **PROS** (3–5 bullets): concrete strengths backed by context.md evidence — specific titles held, domains shipped, recognizable tech. No vague praise.

   **CONS / gaps** (3–5 bullets): missing keywords for target roles, thin areas, anti-preferences that narrow the funnel, seniority signals that are unclear or contradictory.

   **Market positioning** (one paragraph): which role family and seniority the profile slots into most credibly given Strengths + Experience-domains. Name 2–3 role titles that are the best fit; name 1–2 that are stretch goals requiring profile work.

   **Quick wins** (2–3 bullets): specific, actionable edits to the LinkedIn profile or `context.md` — e.g. "surface X project in About section", "add Y keyword to headline", "flip Anti-preferences to clarify you're open to Z".

Anti-pattern: do not restate what context.md says verbatim. Synthesize — the value is the interpretation, not the transcription.

## Mode: followup

**Goal:** for every non-terminal job, collect status updates interactively.

Terminal statuses (skip): `rejected`, `withdrawn`, `closed`, `accepted`.
Non-terminal: `active`, `applied`, `interviewing`, `offer`.

1. Read `jobs.md`. Build list of non-terminal jobs sorted by oldest Updated/Logged first (needs attention most).
2. For each, present via `AskUserQuestion`:
   - Header: `"<Company> — <Title>"`
   - Body: current status, days since last update, job link
   - Options: valid next statuses (state machine below) + `"No change"` + `"Add note only"`
3. Apply answer:
   - **Status change** → move row to new section table, set `Updated` = today, overwrite History with `YYYY-MM-DD: status → <new>`.
   - **Add note only** → prompt for note text, overwrite History with `YYYY-MM-DD: <note>`. Do not change `Updated`.
   - **No change** → overwrite History with `YYYY-MM-DD: checked, no change`. Do not change `Updated`. This is required — without it the job re-surfaces tomorrow with no signal it was reviewed.
4. Update `Last followup:` timestamp at top of `jobs.md`.
5. Report: jobs reviewed, status breakdown.

**Non-interactive fallback:** if `AskUserQuestion` unavailable or times out on the first question → write `pending-followup.md` listing jobs that need attention. Do not invent status changes.

**State machine (valid transitions only):**
```
active        → applied | withdrawn | closed
applied       → interviewing | rejected | withdrawn | closed
interviewing  → offer | rejected | withdrawn
offer         → accepted | rejected | withdrawn
```

## Mode: stats

**Goal:** concise picture of hunt health. Read `jobs.md`, output:

1. **Pipeline** — count by section: Active / Applied / Interviewing / Offer / Rejected / Withdrawn / Accepted.
2. **Conversion rates** — applied→interview rate, interview→offer rate. Skip if sample < 5 (too small to be meaningful).
3. **Top companies** — 3 most-applied-to companies.
4. **Time in pipeline** — average days from Logged to Applied (for Applied+ jobs).
5. **Health verdict** (one sentence): should the user search more, apply more, or follow up more? Base it on the pipeline shape — e.g. many Active + few Applied → apply more; many Applied + no followups → follow up more.

Do not invent rates or trends from insufficient data. Note sample size when rates are shown.

## File integrity

Read `jobs.md` whole → modify in memory → write whole (followup mode). Never partial-edit.

## References (at plugin root `references/`)

- `context-md-schema.md` — profile Strengths / Interests / Experience-domains / Anti-preferences
- `jobs-md-schema.md` — jobs.md section structure, row columns, terminal vs non-terminal statuses
