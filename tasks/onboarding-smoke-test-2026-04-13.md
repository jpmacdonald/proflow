# Onboarding Smoke Test

Date: 2026-04-13

## Scope

This was the smallest live Planning Center probe to verify that the real onboarding path is usable in this environment.

Command used:

```bash
cargo run --bin dump_plans -- --days 14
```

Note:

- Running the command inside the sandbox crashed in macOS `system-configuration` before making the network call.
- Re-running the same command outside the sandbox succeeded.

## Result

The probe succeeded and returned real Planning Center data for the next 14 days.

Observed service types:

- `10:30am traditional`
- `8:00AM`
- `9:00am contemporary`
- `Children's Ministries`
- `Christmas Eve`
- `Funerals & Memorial Services`
- `Mid-Week Lent and Other`
- `Retreat Music and Worship`
- `Student Ministries - AM`
- `Sunday Morning Worship | 11:30 outdoor/COVID`
- `Wednesday Night Lenten Sevices`
- `Youth Choir Rehearsal`
- `Youth Group`

Observed plans in scope:

- `9:00am contemporary` for April 19, 2026
- `10:30am traditional` for April 19, 2026
- `Youth Group` for April 19, 2026
- `9:00am contemporary` for April 26, 2026
- `10:30am traditional` for April 26, 2026
- `Youth Group` for April 26, 2026

## Real Patterns Confirmed

These are useful setup signals pulled directly from the live plans:

- `Welcome (Robert)` / `Welcome (Hope)` appears as a recurring speaker-driven graphic item.
- `Call to Worship` appears in the traditional service with `Liturgist:` lines in the description.
- `Prayer and the Lord's Prayer (Elder/Robert)` appears as a recurring speaker-driven item.
- `Scripture (Elder)` / `Scripture (Robert)` appears as a recurring speaker-driven title item.
- `Sermon (Robert)` / `Sermon (Hope)` appears as a recurring speaker-driven title item.
- Traditional services still include static liturgical items like `Doxology and Prayer of Dedication`, `Gloria Patri`, and `Apostles' Creed`.
- Youth Group has a very different shape and should likely remain outside the default weekly profile or use different rules entirely.

## Immediate Product Implications

- The new setup heuristics for speaker-driven bundle rules are justified by real data.
- `Welcome` and `Call to Worship` are good fixture candidates for `expand` rule drafting.
- Service-group and profile defaults should stay scoped; `Youth Group` should not be treated as the same workflow as Sunday worship.
- The next real smoke-test step should run the MCP path itself, not just `dump_plans`.

## Next MCP Smoke-Test Commands

Once running against the MCP server:

1. `catalog_assets`
2. `analyze_recent_plans`
3. `draft_project_config`
4. `write_project_config` with `activate=false`
5. `validate_config`
6. `preview_playlist` for one real April 19 or April 26 Sunday plan
7. `find_unmapped_items`
8. `suggest_config_patch`
9. `apply_config_patch` with `activate=false`

## Fixture Candidates

Save or derive fixture sets for:

- `Welcome (Robert)` / `Welcome (Hope)`
- `Call to Worship` with `Liturgist:` description
- `Prayer and the Lord's Prayer (...)`
- `Scripture (...)`
- one contrasting `Youth Group` plan to prove service-group separation
