# OcelAudit — 90-second walkthrough

A scripted demo that hits every TLP outcome and the full review flow.
Total time on a warm machine: about 90 seconds.

Prereqs: `make demo` is up.

## Beat 1 — sign in (10 s)

Sign in as `compliance / compliance` (the seeded demo credentials).
Land on the Dashboard. KPI cards show the live state — CSL record
count, last refresh, TLP histogram. Note this is *not* a static demo;
every count is a real read against `wasi:filesystem`.

## Beat 2 — RED hit on a real SDN entry (15 s)

Click **Search** in the nav. Type `Acme Holdings Pyongyang` and hit
Enter. Result: one hit, banded RED. The card shows:

- the entry's source code (**SDN**)
- a citation link to the OFAC reference page (opens in a new tab)
- the matched fields (name, alias)
- the audit_id (UUIDv7)
- decision: `pending-block`

Read the result row aloud: "screening flagged Acme Holdings Pyongyang
under SDN, score 1.0, decision pending-block — review required."

## Beat 3 — fuzzy / typo recall (10 s)

Search again with `Bejing Cyber Solutions` (note the typo —
"Beijing" with the `i` missing). Result: still RED. The Jaro-Winkler
reranker picks it up despite the BM25 score being low. Demonstrates
"don't miss the right entry just because the spelling drifts."

## Beat 4 — GREEN no-match (5 s)

Search `Quetzal Insurance Co`. Result: GREEN, no hits. Decision is
`auto-green`; the audit log records it but no review is needed.
This is what 99% of compliance traffic looks like — the system
shouldn't burn a review on every name that touches the API.

## Beat 5 — review queue (15 s)

Click **Review** in the nav. The pending-block from Beat 2 is at the
top. Click "decide", paste a note ("sanctioned entity confirmed"),
click **Block**. The queue shrinks by one.

## Beat 6 — audit detail (15 s)

Click **Audit**. The full log is paginated, newest-first. Click the
RED row from Beat 2. The detail page shows:

- the original SearchEvent
- `initial_decision: pending-block`
- `current decision: blocked`
- the WorkflowEntry from Beat 5, with the note and the
  `decided_by: compliance` line

This is the full-history audit trail PLAN.md M5 calls out as the
"block-on-pending-block" contract.

## Beat 7 — admin (10 s)

Sign out (top-right). Sign in as `admin / admin`. Note the **Admin** nav
link is now visible (compliance role doesn't see it). Click it.
Press **Update CSL now**. The KPI updates. Mention that in
production this would also fire on a schedule via an external cron
hitting the same endpoint — see the README's "WASI P3 caveats" for
why scheduled refresh is out-of-process here.

## Beat 8 — supply chain (10 s)

Mention without doing live: every `.wasm` shipped with this demo is
built by `cargo auditable`. `cargo audit bin <component>.wasm` reads
the embedded dependency tree against the RustSec advisory DB. Each
release tag (`v0.X.0`) carries a SLSA build provenance attestation
verifiable with `gh attestation verify oci://ghcr.io/...:vX`.
Point at the README's "Supply chain and attestations" section.

## Wrap (5 s)

"That's the whole demo. Source is at github.com/cosmonic-labs/ocelaudit;
PR-ready in about an afternoon. Questions?"
