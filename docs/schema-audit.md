# ProFlow schema audit against ProCore descriptors

ProFlow claims byte-fidelity with ProPresenter documents. The authoritative
schema is not the published API documentation; it is the `FileDescriptorProto`
set compiled into the ProCore engine. This audit compares the checked-in protos
in `crates/propresenter/proto/` against those embedded descriptors.

Tooling: `tools/schema-audit` (Rust bin) and `tools/proto-gen` (emits the
descriptor set as a sidecar). The drift gate is `just schema-audit`.

Audited build: ProPresenter 20.0.1 (build 335544582), macOS, audited 2026-08-03.

## Running the audit

1. Regenerate protobufs so the descriptor set sidecar
   (`target/schema-audit/proflow.pb`) is current:

   ```sh
   just proto
   ```

2. Audit a ProPresenter app (or a raw ProCore binary):

   ```sh
   just schema-audit /Applications/ProPresenter.app
   ```

   The tool locates `ProCore.framework/ProCore` under the app, extracts the
   embedded descriptors, compares them against the sidecar, and consults
   `tools/schema-audit/baseline.json`.

3. Interpretation:

   - Exit 0, "audit clean": every divergence between the binary and the
     schema is recorded in the baseline with a note.
   - Exit 1 with `NEW` lines: the binary drifted in a way the baseline does
     not record. Record the intentional divergences (and the app version)
     by regenerating the baseline:
     `cargo run --manifest-path tools/schema-audit/Cargo.toml -- <binary> --baseline-out tools/schema-audit/baseline.json`
   - Exit 1 with `STALE` lines: the baseline records divergences the current
     binary no longer has (the schema caught up). Delete those entries and
     re-run.

4. Record the ProPresenter version, build number, and date beside the report.
   Treat the report as release evidence, as with `docs/release-audit.md`.

## Findings for 20.0.1

### Fixed in this audit

- `rv2d.proto` `RenderLayer`: the field numbers were off by one against the
  binary (bounds=9, flip_mode=10, rotation=11, build_index=12, build_out=13,
  build_out_index=14, not 8-12), `build_out_index` was missing, and the
  `LayerType` oneof was missing the `zone = 8` member. Corrected and
  regenerated.
- `slide.proto` `Slide.Element.DataLink`: missing `key_value_text = 36`,
  `key_value_fill = 37`, and `zone = 38`, plus the `KeyValueText`, `KeyValueFill`,
  and `Zone` messages. Added and regenerated.
- `timers.proto` `Timer.Format.Style`: `STYE_NONE` typo renamed to the binary's
  `STYLE_NONE` (value 0 unchanged).

After these fixes the report shows zero diffs for rv2d, slide, and
presentation. The remaining gaps are recorded in
`tools/schema-audit/baseline.json`, which the audit gate requires.

### Known divergence (32 items) as of the audit

- Whole files not modeled (57): `psbWorkspace.proto`
  (`ProScoreboardWorkspace`), `zone.proto`, `advertisementGroup.proto`,
  `customOptions.proto`, the `proApiV1*`/`proApi.proto` REST family, and the
  `analytics*` telemetry family. Add each only when a feature needs it.
- `proworkspace.proto`: `ProPresenterLocalWorkspace` and the
  `cloud_output_screens`/`cloud_video_inputs`/`cloud_audio_inputs` fields on
  `ProPresenterWorkspace` are missing. Relevant if workspace parsing must
  cover cloud workspaces.
- `proscreen.proto`: the `CloudProPresenterScreen` family and
  `ProPresenterScreen.rendering_enabled` are missing.
- `slide.proto`/`propDocument.proto`/`ccli.proto`: `CCLIDocument.layout`,
  `MacroCollection.macros`, `PropCollection.cues` are missing.
- `screens.proto`: `OutputDisplay.HDR` and `hdr_mode` are missing.
- `input.proto`: `CloudAudioInput` and `CloudVideoInput` are missing.
- `proCore.proto`: binary-only additions are `SlideThumbnailRequest`/
  `SlideThumbnailResponse`, `LeaveBreadcrumb` (+ `Type`, `MetadataEntry`),
  `InputStatusCallback`, `SendData.slide_thumbnail`/`psb_workspace`/
  `input_status_callback`, `SendDataResponse.leave_breadcrumb`/
  `slide_thumbnail`, `MacroIcons.MacroIcon.content_type`, and
  `TriggerTransferRenderState.SlideState.trigger_source` (+ `TriggerSource`).
  Schema-only leftovers from an older release: `CoreDataStateDump`,
  `SendDataResponse.test_state_dump`, `SlideState.from_timeline`. Prune or
  verify these against the next release.
- `proCoreFeatureFlags.proto`: seven flags are schema-only and three are
  binary-only (`hdr_rendering_enabled`, `in_app_notifications_enabled`,
  `server_driven_feature_flags`). This file drifts between builds; re-verify
  per release and treat the enum set as evidence of which build the protos
  were taken from.
- `mediaMetadata.proto`: `BUFFER_FORMAT_NONE` is binary-only.

### Deliberate divergence

- `graphicsData.proto` `Media.Metadata.color_format = 6`, the `ColorFormat`
  enum, and `Media.VideoProperties.hardware_decoding = 7` are not in the
  20.0.1 binary (binary `Media.Metadata` is fields 1-5 only). They came from
  files produced by a newer release. They are wire-safe to keep (unknown
  fields are preserved), but they are not the 20.0.1 contract.

## Caveats

- Field number and name equality is necessary but not sufficient: oneof
  membership, scalar types, and packedness are not compared. Use the
  extractor's `type`/`oneof_index` data to close a specific doubt.
- Descriptors are the app's compiled contract, but they can disagree with the
  wire. Known example: `Presentation.selected_arrangement` is declared
  `UUID` at field 10 in the 20.0.1 descriptor, yet current documents carry a
  varint there; ProFlow keeps the UUID declaration per the descriptor.
  Real-file corpus evidence wins over descriptors when the two conflict.
