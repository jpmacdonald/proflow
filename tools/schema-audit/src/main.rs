//! Schema drift audit against a ProPresenter binary.
//!
//! Compares the checked-in protos (via the descriptor set emitted by
//! proto-gen) against FileDescriptorProtos embedded in the ProCore engine's
//! `__const` section, and fails when a divergence is not covered by the
//! baseline allowlist.

use anyhow::{bail, Context, Result};
use prost::Message as _;
use prost_types::{DescriptorProto, EnumDescriptorProto, FileDescriptorProto, FileDescriptorSet};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Message,
    Enum,
}

struct Sym {
    kind: Kind,
    fields: BTreeMap<String, i32>,
}

type FileIndex = BTreeMap<String, Sym>;
type Index = BTreeMap<String, FileIndex>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffKind {
    FileOnlyBinary,
    FileOnlySchema,
    SymOnlyBinary,
    SymOnlySchema,
    KindMismatch,
    FieldOnlyBinary,
    FieldOnlySchema,
    NumberMismatch,
}

struct Diff {
    file: String,
    id: String,
    line: String,
    kind: DiffKind,
}

#[derive(Serialize, Deserialize)]
struct BaselineEntry {
    id: String,
    note: String,
}

#[derive(Serialize, Deserialize)]
struct Baseline {
    app: String,
    known: Vec<BaselineEntry>,
}

fn blob_starts(data: &[u8]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x0a {
            let len = usize::from(data[i + 1]);
            let name_start = i + 2;
            let string_end = name_start + len;
            if (1..=0x40).contains(&len)
                && string_end < data.len()
                && data[string_end] == 0x12
                && string_end >= name_start + 6
                && data[name_start..string_end].ends_with(b".proto")
                && data[name_start..string_end - 6]
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                starts.push(i);
            }
        }
        i += 1;
    }
    starts
}

fn parse_varint(data: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0;
    while shift <= 63 {
        let b = *data.get(i)?;
        i += 1;
        result |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Some((result, i));
        }
        shift += 7;
    }
    None
}

/// End offset of the flat field list starting at `start`, mirroring the
/// reference scanner: stop at tag 0 (alignment padding) or a bad wire type.
fn flat_message_end(data: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    loop {
        let (tag, next) = parse_varint(data, i)?;
        let field_number = tag >> 3;
        let wire_type = tag & 7;
        if field_number == 0 || !matches!(wire_type, 0 | 1 | 2 | 5) {
            return Some(i);
        }
        i = next;
        match wire_type {
            0 => i = parse_varint(data, i)?.1,
            2 => {
                let (len, next) = parse_varint(data, i)?;
                i = next + usize::try_from(len).ok()?;
            }
            5 => i += 4,
            1 => i += 8,
            _ => return Some(i),
        }
    }
}

fn extract_descriptors(data: &[u8]) -> BTreeMap<String, FileDescriptorProto> {
    let mut files = BTreeMap::new();
    for start in blob_starts(data) {
        let Some(end) = flat_message_end(data, start) else {
            continue;
        };
        let Ok(fdp) = FileDescriptorProto::decode(&data[start..end]) else {
            continue;
        };
        let Some(name) = fdp.name.as_deref() else {
            continue;
        };
        if name.ends_with(".proto") && fdp.package.is_some() && !files.contains_key(name) {
            files.insert(name.to_string(), fdp);
        }
    }
    files
}

fn walk_msg(msg: &DescriptorProto, prefix: &str, out: &mut FileIndex) -> Result<()> {
    let name = msg.name.as_deref().context("message missing name")?;
    let full = format!("{prefix}{name}");
    let mut fields = BTreeMap::new();
    for f in &msg.field {
        let field_name = f.name.as_deref().context("field missing name")?;
        let number = f.number.context("field missing number")?;
        fields.insert(field_name.to_string(), number);
    }
    out.insert(
        full.clone(),
        Sym {
            kind: Kind::Message,
            fields,
        },
    );
    for nested in &msg.nested_type {
        walk_msg(nested, &format!("{full}."), out)?;
    }
    for e in &msg.enum_type {
        walk_enum(e, &format!("{full}."), out)?;
    }
    Ok(())
}

fn walk_enum(e: &EnumDescriptorProto, prefix: &str, out: &mut FileIndex) -> Result<()> {
    let name = e.name.as_deref().context("enum missing name")?;
    let full = format!("{prefix}{name}");
    let mut fields = BTreeMap::new();
    for v in &e.value {
        let value_name = v.name.as_deref().context("enum value missing name")?;
        let number = v.number.context("enum value missing number")?;
        fields.insert(value_name.to_string(), number);
    }
    out.insert(
        full,
        Sym {
            kind: Kind::Enum,
            fields,
        },
    );
    Ok(())
}

fn index_files<'a>(
    files: impl Iterator<Item = (String, &'a FileDescriptorProto)>,
) -> Result<Index> {
    let mut out = Index::new();
    for (name, fdp) in files {
        let mut fi = FileIndex::new();
        for m in &fdp.message_type {
            walk_msg(m, "", &mut fi)?;
        }
        for e in &fdp.enum_type {
            walk_enum(e, "", &mut fi)?;
        }
        out.insert(name, fi);
    }
    Ok(out)
}

fn compute_diffs(bin: &Index, schema: &Index) -> Vec<Diff> {
    let mut diffs = Vec::new();
    let files: BTreeSet<&String> = bin.keys().chain(schema.keys()).collect();
    for file in files {
        match (bin.get(file), schema.get(file)) {
            (Some(_), None) => diffs.push(Diff {
                file: file.clone(),
                id: file.clone(),
                line: file.clone(),
                kind: DiffKind::FileOnlyBinary,
            }),
            (None, Some(_)) => diffs.push(Diff {
                file: file.clone(),
                id: file.clone(),
                line: file.clone(),
                kind: DiffKind::FileOnlySchema,
            }),
            (None, None) => continue,
            (Some(b), Some(s)) => {
                let syms: BTreeSet<&String> = b.keys().chain(s.keys()).collect();
                for name in syms {
                    match (b.get(name), s.get(name)) {
                        (Some(_), None) => diffs.push(Diff {
                            file: file.clone(),
                            id: format!("{file}::{name}"),
                            line: format!("  {name}: in binary, NOT in schema"),
                            kind: DiffKind::SymOnlyBinary,
                        }),
                        (None, Some(_)) => diffs.push(Diff {
                            file: file.clone(),
                            id: format!("{file}::{name}"),
                            line: format!("  {name}: in schema, NOT in binary"),
                            kind: DiffKind::SymOnlySchema,
                        }),
                        (None, None) => continue,
                        (Some(bs), Some(ss)) => {
                            if bs.kind != ss.kind {
                                diffs.push(Diff {
                                    file: file.clone(),
                                    id: format!("{file}::{name}"),
                                    line: format!(
                                        "  {name}: kind schema={:?} binary={:?}",
                                        ss.kind, bs.kind
                                    ),
                                    kind: DiffKind::KindMismatch,
                                });
                                continue;
                            }
                            let flds: BTreeSet<&String> =
                                bs.fields.keys().chain(ss.fields.keys()).collect();
                            for fld in flds {
                                let fq = format!("{file}::{name}.{fld}");
                                match (bs.fields.get(fld), ss.fields.get(fld)) {
                                    (Some(_), None) => diffs.push(Diff {
                                        file: file.clone(),
                                        id: fq,
                                        line: format!(
                                            "  {name}.{fld}: binary only (schema missing field)"
                                        ),
                                        kind: DiffKind::FieldOnlyBinary,
                                    }),
                                    (None, Some(_)) => diffs.push(Diff {
                                        file: file.clone(),
                                        id: fq,
                                        line: format!("  {name}.{fld}: schema only"),
                                        kind: DiffKind::FieldOnlySchema,
                                    }),
                                    (Some(&bv), Some(&sv)) if bv != sv => diffs.push(Diff {
                                        file: file.clone(),
                                        id: fq,
                                        line: format!(
                                            "  {name}.{fld}: NUMBER schema={sv} binary={bv}"
                                        ),
                                        kind: DiffKind::NumberMismatch,
                                    }),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    diffs
}

fn auto_note(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::FileOnlyBinary => "binary-only file",
        DiffKind::FileOnlySchema => "schema-only file",
        DiffKind::SymOnlyBinary => "in binary, not in schema",
        DiffKind::SymOnlySchema => "in schema, not in binary",
        DiffKind::KindMismatch => "kind mismatch",
        DiffKind::FieldOnlyBinary => "binary only",
        DiffKind::FieldOnlySchema => "schema only",
        DiffKind::NumberMismatch => "field number mismatch",
    }
}

fn print_report(bin_files: usize, schema_files: usize, diffs: &[Diff]) {
    println!("binary descriptor files: {bin_files}, schema files: {schema_files}");
    let file_level: Vec<&Diff> = diffs
        .iter()
        .filter(|d| matches!(d.kind, DiffKind::FileOnlyBinary | DiffKind::FileOnlySchema))
        .collect();
    if !file_level.is_empty() {
        println!("\n=== FILE-LEVEL DIVERGENCES ===");
        for d in &file_level {
            println!("  {}", d.line);
        }
    }
    let mut per_file: BTreeMap<&str, Vec<&Diff>> = BTreeMap::new();
    for d in diffs
        .iter()
        .filter(|d| d.kind != DiffKind::FileOnlyBinary && d.kind != DiffKind::FileOnlySchema)
    {
        per_file.entry(&d.file).or_default().push(d);
    }
    if !per_file.is_empty() {
        println!("\n=== DIFFS (fully-qualified names, field number+name) ===");
        for (file, items) in &per_file {
            println!("--- {file}");
            for d in items {
                println!("  {}", d.line);
            }
        }
    }
}

fn plist_string<'a>(data: &'a [u8], key: &[u8]) -> Option<&'a str> {
    let key_pos = data.windows(key.len()).position(|w| w == key)?;
    let after = &data[key_pos + key.len()..];
    let start = after.windows(8).position(|w| w == b"<string>")? + 8;
    let rest = &after[start..];
    let end = rest.windows(9).position(|w| w == b"</string>")?;
    std::str::from_utf8(&rest[..end]).ok()
}

fn plist_version(info_plist: &Path) -> Option<String> {
    let data = fs::read(info_plist).ok()?;
    let short = plist_string(&data, b"CFBundleShortVersionString")?;
    match plist_string(&data, b"CFBundleVersion") {
        Some(build) => Some(format!("{short} (build {build})")),
        None => Some(short.to_string()),
    }
}

fn probe_binary(path: &str) -> Result<(PathBuf, Option<String>)> {
    let p = Path::new(path);
    if p.is_dir() {
        let candidates = [
            p.join("Contents/Frameworks/ProCore.framework/ProCore"),
            p.join("Contents/Frameworks/ProCore.framework/Versions/A/ProCore"),
            p.join("ProCore.framework/ProCore"),
        ];
        for c in &candidates {
            if c.is_file() {
                let version = plist_version(&p.join("Contents/Info.plist"));
                return Ok((c.to_path_buf(), version));
            }
        }
        bail!("no ProCore binary found under {}", p.display());
    }
    if p.is_file() {
        return Ok((p.to_path_buf(), None));
    }
    bail!("binary path does not exist: {}", p.display());
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .context("resolve repository root from tool manifest path")
}

fn usage() -> String {
    "usage: schema-audit <ProPresenter.app | ProCore binary> [--fds path] \
     [--baseline path] [--baseline-out path] [--version string]"
        .to_string()
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut binary_path: Option<String> = None;
    let mut fds_arg: Option<PathBuf> = None;
    let mut baseline_arg: Option<PathBuf> = None;
    let mut baseline_out: Option<PathBuf> = None;
    let mut version_override: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--fds" => {
                i += 1;
                fds_arg = Some(args.get(i).context("--fds needs a path")?.into());
            }
            "--baseline" => {
                i += 1;
                baseline_arg = Some(args.get(i).context("--baseline needs a path")?.into());
            }
            "--baseline-out" => {
                i += 1;
                baseline_out = Some(args.get(i).context("--baseline-out needs a path")?.into());
            }
            "--version" => {
                i += 1;
                version_override = Some(args.get(i).context("--version needs a value")?.clone());
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return Ok(());
            }
            other if other.starts_with('-') => bail!("unknown option {other}"),
            other if binary_path.is_none() => binary_path = Some(other.to_string()),
            other => bail!("unexpected argument {other}"),
        }
        i += 1;
    }

    let binary_arg = binary_path.context(usage())?;
    let (binary_file, detected_version) = probe_binary(&binary_arg)?;
    let version = match (version_override, detected_version) {
        (Some(v), _) => v,
        (None, Some(v)) => v,
        (None, None) => "unknown".to_string(),
    };

    let repo_root = repo_root()?;
    let fds_path = match fds_arg {
        Some(p) => p,
        None => repo_root.join("target/schema-audit/proflow.pb"),
    };
    let fds_data = fs::read(&fds_path).with_context(|| {
        format!(
            "descriptor set {} not found; run `just proto` first",
            fds_path.display()
        )
    })?;
    let fds = FileDescriptorSet::decode(&fds_data[..])
        .context("decode descriptor set from proto-gen output")?;

    let binary_data =
        fs::read(&binary_file).with_context(|| format!("read binary {}", binary_file.display()))?;
    let extracted = extract_descriptors(&binary_data);
    let bin = index_files(extracted.iter().map(|(n, f)| (n.clone(), f)))?;
    let schema = index_files(
        fds.file
            .iter()
            .map(|f| {
                f.name
                    .clone()
                    .context("descriptor file missing name")
                    .map(|n| (n, f))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter(),
    )?;

    let diffs = compute_diffs(&bin, &schema);
    print_report(extracted.len(), fds.file.len(), &diffs);

    let baseline_path = match baseline_arg {
        Some(p) => p,
        None => repo_root.join("tools/schema-audit/baseline.json"),
    };

    if let Some(out_path) = baseline_out {
        let baseline = Baseline {
            app: version,
            known: diffs
                .iter()
                .map(|d| BaselineEntry {
                    id: d.id.clone(),
                    note: auto_note(d.kind).to_string(),
                })
                .collect(),
        };
        fs::write(&out_path, serde_json::to_string_pretty(&baseline)? + "\n")
            .with_context(|| format!("write baseline {}", out_path.display()))?;
        println!(
            "\nwrote baseline with {} entries to {}",
            diffs.len(),
            out_path.display()
        );
        return Ok(());
    }

    let baseline_data = fs::read(&baseline_path).with_context(|| {
        format!(
            "baseline {} not found; generate it with --baseline-out",
            baseline_path.display()
        )
    })?;
    let baseline: Baseline = serde_json::from_slice(&baseline_data)
        .with_context(|| format!("parse baseline {}", baseline_path.display()))?;
    let known: BTreeSet<&str> = baseline.known.iter().map(|e| e.id.as_str()).collect();
    let current: BTreeSet<&str> = diffs.iter().map(|d| d.id.as_str()).collect();
    let new: Vec<&str> = current.difference(&known).copied().collect();
    let stale: Vec<&str> = known.difference(&current).copied().collect();

    println!("\napp: {version}");
    println!(
        "baseline: {} ({} entries, app {})",
        baseline_path.display(),
        baseline.known.len(),
        baseline.app
    );
    println!(
        "divergences: {} total, {} new, {} stale",
        diffs.len(),
        new.len(),
        stale.len()
    );
    for id in &new {
        let d = diffs
            .iter()
            .find(|d| d.id == *id)
            .context("internal error: computed diff id missing from diff list")?;
        println!("NEW   {id}: {}", d.line.trim());
    }
    for id in &stale {
        println!("STALE baseline entry (no longer diverges): {id}");
    }

    if !new.is_empty() || !stale.is_empty() {
        bail!("schema drift detected; update baseline.json to record intentional divergences");
    }
    println!("audit clean");
    Ok(())
}
