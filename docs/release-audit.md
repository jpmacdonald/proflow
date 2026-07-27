# ProFlow 1.0 ProPresenter release audit

This external gate proves that the current ProPresenter release accepts a
reviewed ProFlow package and preserves its semantic contract after save-back.
It is intentionally separate from deterministic CI because ProPresenter is a
proprietary interactive application.

## Required package audit

1. Build one contemporary and one traditional portable playlist in the safe
   Documents workspace.
2. Import both into the current ProPresenter release and inspect them in the UI.
3. Export each imported playlist again without editing it. Keep these as the
   `saved-back` files; do not use the live Dropbox library for this audit.
4. Run this command for each pair:

   ```sh
   just release-audit generated.proplaylist saved-back.proplaylist
   ```

The command exits successfully only when playlist items, presentation
structures, arrangements, media membership, and package shape remain
semantically compatible. Its JSON output is the release evidence. Record the
ProPresenter version and operating system beside that output.

An export made after operator QA edits is useful as a changed-behavior fixture,
but it is not a roundtrip fixture. Keep it under a different name. The required
audit must be an immediate no-edit export so any presentation-byte difference
is attributable to import/save-back rather than a theme reapplication, text
edit, or other operator action.

Current ProPresenter 21.4 evidence preserved all 22 embedded presentation
members byte-for-byte in an immediate no-edit export. ProPresenter rebuilt the
playlist document around those files: item names used each presentation's
native name, unset item keys became C major, and the synthetic root retained
the active show's expansion state. ProFlow reproduces those playlist defaults
from the checked workflow and live metadata snapshot.

## Optional thumbnail oracle

Export or capture one approved and one post-import cue thumbnail tree using the
same ProPresenter version, Look, output resolution, and filenames. Then run:

```sh
just release-audit generated.proplaylist saved-back.proplaylist approved-thumbnails actual-thumbnails
```

Images are decoded before comparison, so PNG/JPEG container metadata does not
create noise. Membership, dimensions, and every RGBA pixel must match. Baseline
updates are reviewed behavior changes; the command never rewrites them.

## Release record

Keep the following together for every audited release:

- ProFlow commit and build-receipt revision
- ProPresenter version/build and macOS version
- original generated package and ProPresenter saved-back export
- `release_audit` JSON output
- optional approved/actual thumbnail trees and the Look/output configuration
