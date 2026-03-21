# Generated Protobuf Workflow

`rv_data.rs` is generated from the `.proto` files in `src/propresenter/proto`.

Regenerate it with:

```bash
cargo run --manifest-path tools/proto-gen/Cargo.toml
```

Verify the checked-in file is current with:

```bash
cargo run --manifest-path tools/proto-gen/Cargo.toml -- --check
```

The generator uses vendored `protoc` so it does not depend on a system protobuf installation.
