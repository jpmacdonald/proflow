# Text Fit Strategy

Status: proposal. Source: reverse engineering of ProPresenter 20.0.1
(rvtext engine in ProCore.framework, x86_64 slice).

## What ProPresenter does

Overflow is a runtime side effect of the layout pass, not a static analysis.
There is no "measure" step and no answer stored in the file. The engine
re-derives the fit at display time, from the fonts installed on the
rendering machine. Even ProPresenter's own fit answer is
environment-dependent (font substitution changes it).

Mechanism (rvtext::ArrangedText):

1. `SetMaxSize(boxSize)` then `Arrange(width, height, scale, &outHeight,
   &outOverflowed)` runs a full layout: paragraphs -> lines -> ICU bidi
   shaping -> per-character runs.
2. `StartNewLine` (per new line): projected line bottom = current layout
   bottom - line top. If it exceeds the box height, overflow fires:
   - didOverflow = true
   - overflowCharacterIndex = first character that did not fit
   - overflowBreakIndex = word-boundary break, or hard character break
     when overflow mode == scale-to-fit
3. FitText = arrange -> check flag -> shrink per-run font scale ->
   re-arrange, repeated. Not analytic; it is a loop.
4. App consumes the flag via RVGraphicText.isOverflowing (editor red
   indicator, BibleSlideGenerationOperation auto-fit loop).

## What this means for proflow

The file declares intent (box bounds, margins, scaleBehavior); the answer
is computed at render time. Two consequences:

1. If a template declares scale-to-fit, any content fits by definition;
   no fit test is needed for generation.
2. Only templates with fit disabled need a fit test to split text across
   slides reliably.

## Proposed simulation for fit-off templates

Deterministic, self-contained, mirrors the engine structure:

1. Read text element bounds + template margins + scaleBehavior from the
   file (all available statically).
2. Shape with a real shaping engine (swash / font-kit in Rust, or CoreText
   on macOS). Bundle the exact font files with the generator so results are
   deterministic.
3. Wrap at box width; accumulate line bottoms; overflow when
   lineBottom > boxHeight, exactly like StartNewLine.
4. Split at the overflow point with a safety margin (5-10% of box height)
   to absorb renderer differences.

## Open questions

- Which templates in production use fit-off? (decides whether the oracle
  is needed at all)
- Font matching: what does the render machine substitute when fonts are
  missing, and can we pin fonts to avoid divergence?
- tools/text-fit-oracle: current status, and whether it already does the
  shaped-layout simulation.
