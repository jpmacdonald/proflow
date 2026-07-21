# Rendering reliability roadmap

ProFlow is complete only when a reviewed Planning Center plan can be compiled
into native ProPresenter presentations and a playlist package without relying
on operator discovery of structural or styling mistakes, or overflow in
generated and safely measurable restyled text cues.

The reliability model follows the product pipeline:

```text
Planning Center snapshot
  -> semantic plan
  -> destination-aware layout
  -> native presentations
  -> native playlist package
  -> validation evidence
  -> checked playlist-last process transaction
```

## Status

| Stage | Status | Evidence |
|---|---|---|
| Native text fit | Implemented | Embedded TextKit helper, exact final-RTF measurement, metric-run evidence, immutable font-program identity, final-cue postcondition, typed mode failures |
| Audience destinations | Implemented | UUID-only macro -> Look -> enabled screen -> source/theme resolution and destination fit checks; nested macro execution fails closed |
| Preview/build binding | Implemented | Canonical Planning Center revision, immutable asset fingerprint, source/artifact digests, sealed build receipt |
| Independent native parity | Active | Byte-exact native corpus, production-writer template reconstruction, portable-media/link shape audits, read-only live-library/export audits |
| ProPresenter visual parity | Future optional oracle | Post-import/thumbnail comparison remains useful for detecting proprietary compositor changes; it is not part of deterministic CI |

The first three stages form the unattended-build boundary. The fourth guards
the reverse-engineered file contract. The fifth would add application-rendered
visual evidence, but does not replace the deterministic checks above.

## Automated operating contract

An unattended build is accepted only when all of these proofs succeed in one
run:

1. Two consecutive direct Planning Center reads normalize to the same snapshot.
2. Every plan item has one deterministic disposition; ambiguity is review, not
   an inferred best match.
3. Every generated text cue fits its source theme and every enabled Audience
   Look destination selected by its macro.
4. Every restyled text cue accepted for automated layout proof has an
   unambiguous source text element and destination slot. Paint-only color runs
   are geometry-neutral; mixed metric-affecting runs under a theme override and
   structurally richer files require review.
5. The playlist package cross-validates presentation identities, selected
   arrangements, traversal order, and portable media dependencies. Its receipt
   records every reference-to-member mapping, unresolved locator, and warning.
6. The normalized plan, native assets, source files, staged artifacts, text-fit
   contract, and cue evidence are sealed into one deterministic receipt.
7. Planning Center, native assets, CoreText-resolved font programs, sources,
   outputs, and staged bytes are checked again before the playlist-last process
   transaction commits.

Failures are typed at the phase that owns the invariant. A missing macro cannot
become a rendering warning, an overflowing cue cannot become a playlist entry,
and a changed source cannot cross the prepared-build boundary.

This proves the file and text-layout behavior ProFlow implements. It does not
claim a byte-for-byte clone of ProPresenter's private bitmap compositor. A
current-version ProPresenter import remains the independent release oracle for
application-only effects and for detecting future renderer changes.

## 1. Prove rendered text fits

**Goal:** replace character-count layout guesses with native macOS text layout
using the exact attributed text, font, bounds, margins, paragraph settings, and
scale behavior carried by the ProPresenter theme slide.

Completion criteria:

- A native text-fit oracle returns used bounds, line count, visible range, and
  overflow status for one checked text container.
- The scripture and liturgy partitioners use that oracle as their fit predicate;
  grammar and front-loading remain deterministic tie-breakers.
- Every final generated cue is remeasured as a postcondition. Restyled existing
  cues are measured only when their one source field and one destination slot
  can be proven without semantic guessing; richer shapes require review.
- Missing fonts, unsupported text settings, and an unavailable native oracle are
  typed build failures. Production does not silently fall back to estimates.
- The configured maximum line count remains a readability constraint even when
  more glyphs would physically fit.

## 2. Prove every audience destination

**Goal:** validate what each projector and stream output will render, rather than
assuming the editor-visible theme is the only layout.

Completion criteria:

- Macro actions resolve to their Audience Look without name guessing.
- The Look resolves every enabled screen to its alternate presentation theme.
- Text fit succeeds for the source slide and for every resolved destination
  theme used by the macro.
- Theme slides, macros, Looks, screens, presentation size, font programs, and
  background bytes are captured across the immutable render-asset snapshot and
  the reviewed source/font snapshots that own those bytes.
- Conflicting or incomplete native references fail before any output is staged.

## 3. Bind preview, render, and commit

**Goal:** make the approved preview a reproducible build input, not an informal
moment in time.

Completion criteria:

- A canonical Planning Center snapshot is accepted only after two consecutive
  direct normalized reads agree, then checked again immediately before commit.
- Every consumed file and relevant installed render asset has a SHA-256 digest.
- Every staged presentation and playlist package is structurally validated and
  fingerprinted before commit.
- A machine-readable build receipt records the complete normalized plan and
  revision, exact producer metadata, config revision,
  native asset revision, artifact digests, cue counts, macro transitions,
  backgrounds, labels, arrangements, and layout evidence.
- The receipt is committed in the same reviewed process transaction as the
  artifacts it describes, with the playlist installed last as the completion
  marker. Power-loss durability is explicitly outside this boundary.

## 4. Maintain independent native parity

**Goal:** catch mistakes in assumptions about the proprietary file and package
formats without making ProPresenter itself part of deterministic CI.

Completion criteria:

- Current-version, independently exported song, scripture, liturgy, graphic,
  arrangement, and portable-playlist fixtures exercise the production codec.
- Round trips prove byte fidelity where bytes are expected to survive and
  semantic fidelity where ProFlow intentionally generates new identities.
- Native fixture updates are explicit behavior changes, never automatic test
  churn.

## 5. Maintain an optional ProPresenter visual oracle

**Goal:** detect changes in ProPresenter's private compositor and application
behavior without confusing screenshots with deterministic file correctness.

Completion criteria:

- A read-only post-import audit compares the installed library document with the
  build receipt.
- When ProPresenter is running, an optional QA harness captures cue thumbnails
  through its API and compares them with approved render baselines.
- Visual-baseline updates are explicit behavior changes, never automatic test
  churn.

## Verification snapshot — 2026-07-20

- `just local`, `just ci`, and `just deep` pass. The deep gate includes the
  authoritative protobuf check, native parity suite, property tests with 10,000
  cases, semantic invariant tests, and mutation testing of the transaction
  boundary.
- Transaction mutation testing evaluated 50 mutations: 39 were caught by tests,
  10 were rejected as uncompilable, one EOF-loop mutation was killed by timeout,
  and none survived.
- The read-only Default-library audit round-tripped 2,033 native presentations
  byte-for-byte plus its raw playlist document. Five unrelated `.pro`-suffixed
  non-presentation files were identified and excluded by format.
- The read-only Desktop corpus audit round-tripped 9 independently exported
  playlist packages and 126 embedded presentations byte-for-byte. Its media
  audit found 72 exact locator matches and 130 unique-basename rebase matches;
  those rebases are package-shape evidence, not proof of application relocation
  or import behavior.

The live Planning Center smoke test and a current-version ProPresenter
import/render comparison remain intentionally separate external gates.

## Delivery order

1. Native text-fit oracle and final-cue overflow postcondition.
2. Macro -> Look -> screen-theme resolution and font/scale preflight.
3. Planning Center and render-asset revisions plus sealed build receipts.
4. Cue-level QA reports and current-version native goldens.
5. Optional post-import and thumbnail parity harnesses.

The first three stages are the unattended-build boundary. Stages four and five
are independent evidence that the boundary continues to match ProPresenter as
the application evolves.
