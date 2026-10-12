# Review decisions

A plan's decisions are one file:

```text
<PROFLOW_DATA>/builds/<plan id>/review.toml
```

It holds everything an operator chooses for that plan. `proflow build` and the
MCP `preview_playlist` tool both review the plan with exactly what this file
says; `preview_playlist` writes it from its arguments, replacing what was
there. A plan with no file has no decisions.

An item the review cannot resolve is an open decision. The build lists every
open decision, exits with status 2, and renders nothing until each one has an
entry here.

## Recording a decision

Under each open decision the build prints the choices it can offer and the
command that records each one:

```text
  pco:101:main  Amazing Grace
      Configured existing song target is ambiguous: Amazing Grace.pro
      - use /Library/Amazing Grace.pro:
          proflow decide --plan 12345 --item pco:101:main --use '/Library/Amazing Grace.pro'
      - use /Library/Archive/Amazing Grace.pro:
          proflow decide --plan 12345 --item pco:101:main --use '/Library/Archive/Amazing Grace.pro'
      - leave it out of the playlist:
          proflow decide --plan 12345 --item pco:101:main --skip
```

The choices come from what the review found: the library files it could not
choose between, the arrangements the chosen file defines, or the action it
proposed. Leaving the item out is always offered. Each one is checked against
the plan before it is printed.

`proflow decide` writes the entry described below, replacing any earlier
decision for the same item. It takes every kind of decision, not only the
ones a build offers:

| Decision | Options |
|---|---|
| Leave the item out | `--skip` |
| Use a library presentation | `--use <file.pro> [--arrangement <name>]` |
| Keep the proposed presentation, select an arrangement | `--arrangement <name>` |
| Render the item as a new presentation | `--generate [--background <id>]` |
| Render the description into a presentation | `--edit <file.pro> [--background <id>]` |
| Keep the proposed render, change its background | `--background <id>` |

`decide` does not fetch the plan. An item the plan does not have is reported
by the next build.

## A different image for a background, for one plan

A background is configured once, by id, in `proflow.config.json`. To use
another image for it in one plan, such as this week's sermon graphic:

```bash
proflow background --plan 12345 --id sermon --file backgrounds/2026-10-18.png
```

Every item in that plan whose background is `sermon` then uses that file.
The file is a jpg, png or tiff below the data root; the id must be a
configured background. Other plans and the configuration are unchanged.
`--reset` in place of `--file` goes back to the configured image. In
`review.toml` it is:

```toml
[backgrounds]
sermon = "backgrounds/2026-10-18.png"
```

MCP `preview_playlist` takes the same mapping as `background_files`.

## Remembered choices and defaults

Some questions come back. A question is an item's title and what is being
asked about it (which file, which arrangement, whether to take a proposal, or
what to do with an item nothing matches), so "Amazing Grace, which file?" is
the same question in next month's plan.

**Remembered.** When you answer a question a build asked, the choice is
remembered. The next time the question is asked, the choices made before are
listed first, most recent first, with how often and when:

```text
      - use /Library/Amazing Grace.pro (chosen 3x, last 2026-10-05):
```

The question is still asked. Nothing is chosen for you.

**Default.** Add `--default` to a `decide` command to have that choice made
for the same question from then on:

```bash
proflow decide --plan 12345 --item pco:101:main --use '/Library/Amazing Grace.pro' --default
```

A later build that meets the question records the default in that plan's
`review.toml`, marked `decided_by = "default"`, and reports it:

```text
Answered with your default:
  pco:101:main  Amazing Grace: use /Library/Amazing Grace.pro
```

A default the plan does not accept (the file is gone, for example) is taken
back out of `review.toml` and reported, and the question is asked as usual.
To override a default for one plan, run `decide` for that item; a decision
recorded for the plan always wins.

Both live in `<PROFLOW_DATA>/decision-memory.toml`. A default can only be set
for a question the plan's latest build asked. There is no command yet to list
or clear defaults; remove the `default` table of a question in that file.

## The file

```toml
playlist_name = "May 24, 2026 - Sunday Morning"   # default: the plan's date and service type
package_mode = "export_portable"                  # default; or "library_local"

[[media]]                                         # extra files for a portable package
path = "/Users/me/Media/welcome.mp4"

[[decision]]
item = "pco:101:main"
skip = true

[[decision]]
item = "pco:102:main"
playlist_name = "Hymn of Response"
slide_type = "lyrics"

[decision.action]
kind = "use_existing"
file_path = "~/Documents/ProPresenter/Libraries/Default/Amazing Grace.pro"
arrangement = "Short"
```

`item` is the output key the build printed. Each item may have one decision.

A decision either sets `skip = true`, which leaves the item out of the
playlist, or changes the item with any of:

- `playlist_name`: the item's name in the playlist.
- `slide_type`: `text`, `lyrics`, `scripture`, `title`, `graphic`, or `nametag`.
- `[decision.action]`: what to do for the item, by `kind`:

| `kind` | Fields | Effect |
|---|---|---|
| `use_existing` | `file_path`, optional `arrangement` | Use this library presentation unchanged. |
| `generate_new` | optional `background` | Render the item's content as a new presentation. |
| `edit_description` | `file_path`, optional `background` | Render the item's description into this presentation. |
| `set_background` | `background` | Keep the proposed render; use this background. |
| `select_arrangement` | `arrangement` | Keep the proposed presentation; select this arrangement. |

`background` is a background id from the project config.

Four more fields may be written on a decision for whoever reads the file
later: `question`, `options` (a list), `decided_by` (`operator` or `rule`),
and `at` (a timestamp). They do not change what is built.

Unknown fields are errors, so a misspelled key is reported instead of ignored.

## What changing the file does

The file's decisions are one of the five inputs a build is named by, with the
plan as Planning Center has it, the configuration and display assets, the
Bible corpora, and the presentation library. `proflow commit` and the MCP
`build_service` tool install the verified build of those inputs as they are
at that moment. Edit a decision after building and there is no build of the
new decisions to install: build again.
