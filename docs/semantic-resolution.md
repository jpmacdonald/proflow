# Semantic resolution contracts

These contracts govern translation, song identity, and song arrangements.
The existing compiler pipeline remains unchanged.
Each resolver owns one decision before native rendering starts.

## Common rules

- A default fills an omitted value. It never replaces an invalid explicit value.
- A configured override states authority before resolution. It is not a recovery path after a failed match.
- A candidate is evidence for review. It cannot become an executable source through a styling decision.
- Ambiguous matches require review. File order and search scores cannot establish identity.
- Rendering consumes resolved decisions. It cannot choose a replacement value.

## Translation

The Scripture resolver in `src/workflow/scripture.rs` owns translation resolution.
The classifier reconciles title declarations with explicit structured translation data.

| Input | Result |
|---|---|
| One supported explicit translation | Preserve that translation. |
| No explicit translation | Use the configured default, or require review if no default exists. |
| An unsupported explicit translation | Require review, regardless of the configured default. |
| Conflicting declarations for one passage | Require review. |
| Several passages with one explicit translation | Apply that translation to passages without a declaration. |
| Several explicit translations | Require an explicit translation on every passage. |

An explicit structured translation and an explicit title translation must agree.
The configured default has no authority over either declaration.
Partial verses and selected excerpts obey the same rules before excerpt reconciliation.

A parenthetical speaker annotation requires a configured person.
An unknown annotation cannot disappear as an assumed speaker.
The parser retains the existing selected-verse directives as content syntax.

The normalized source can contain Scripture metadata inferred from its title.
That inferred reference is not independent authority over the complete title.
This distinction preserves multiple passages in one item.

The executable plan carries `BibleVersion` through capture and rendering.
Checked constructors require canonical references without embedded translation names or annotations.
Consumers do not parse the translation name again.
The serialized translation retains its existing spelling.

## Song identity

The song resolver in `src/workflow/library_search.rs` owns automatic library identity.
The song classifier supplies original titles instead of preprocessed identity claims.

An attached Planning Center song title supplies the primary title.
Without an attached song, the item title supplies the primary title.
An exact item-title match to another file creates a conflict.
The item title cannot replace an unmatched qualified song title.

Automatic identity requires one full filename match, excluding the `.pro` extension.
Comparison ignores letter case and repeated whitespace.
It preserves punctuation, parentheticals, bracketed qualifiers, and title words.
Duplicate matching files require review.

Prefix removal, hymn-number removal, qualifier removal, fuzzy matching, and role scores can produce suggestions only.
An exact configured `library_file` is an explicit project selection.
It must resolve to one file and never falls back to fuzzy search.
A reviewed `UseExisting` decision supplies an explicit operator selection.

A song candidate retains its proposed transformation for review.
Its typed review state still requires source selection.
An arrangement or background decision cannot satisfy that requirement.
An explicit source selection preserves the proposed transformation.

## Song arrangements

The song classifier in `src/workflow/classify/song.rs` owns arrangement resolution.
It selects the request through this fixed authority order:

1. An explicit per-entry operator decision.
2. A matching configured service override.
3. The configured presentation arrangement.
4. The Planning Center arrangement.
5. Native default selection, only when no request exists.

Configured arrangements are explicit project overrides.
Configuration rejects conflicting service overrides that can match the same item.
Reordering those overrides cannot select a different arrangement.

| Request | Native evidence | Result |
|---|---|---|
| An explicit name | One complete name match | Select the canonical native name. |
| An explicit name | Missing, ambiguous, or incomplete match | Require review. |
| Planning Center `Default Arrangement` | No exact match, one complete `Default` | Apply the documented alias. |
| Planning Center `Default Arrangement` | No native arrangements | Preserve the unarranged presentation. |
| No request | One complete `Default` | Select `Default`. |
| No request | No native arrangements | Preserve the unarranged presentation. |
| No request | Other native arrangements | Require review. |

Name comparison ignores letter case and retains the canonical native spelling.
An exact `Default Arrangement` match precedes the alias.
The alias does not apply to configured or operator-supplied names.
A sole arrangement with another name does not become a default.
An unresolved arrangement retains a typed review obligation.
A background change or a later review check cannot discharge that obligation.
An explicit arrangement or source selection is required.

Native preparation checks the selected name, identity, and traversal again.
An operator decision cannot bypass those native checks.

## Regression evidence

Classifier tests exercise the complete `build_plan` path.
Translation tests cover explicit declarations, defaults, conflicts, annotations, and excerpts.
Song tests separate full-title identity from suggestions and conflicting source titles.
Arrangement tests cover authority, exact names, the narrow alias, and unavailable names.
Override tests prove that styling cannot approve an unresolved song identity or arrangement.
Property tests exercise default noninterference and rejection of unavailable arrangement names.

The completion gate is `just deep`.
