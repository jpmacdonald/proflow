//! Immutable Bible corpus snapshots and freshness proofs.

use super::{version, BibleVersion};
use super::{
    BTreeMap, BibleCorpusError, BibleData, CachedBibleData, HashMap, PathBuf, ScriptureHeader,
    ScriptureLookupError, ScriptureRef, Sha256, Verse, VerseSelection,
};
use sha2::Digest;
use std::fmt::Write;

/// Content identity of one captured Bible corpus set: the data root and the
/// SHA-256 of every installed translation's source bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibleCorpusFingerprint {
    root: PathBuf,
    sources: BTreeMap<String, [u8; 32]>,
}

impl BibleCorpusFingerprint {
    /// The data root and every installed translation's source SHA-256.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "root": self.root,
            "sources": self
                .sources
                .iter()
                .map(|(version, sha256)| (version.as_str(), digest_hex(sha256)))
                .collect::<BTreeMap<_, _>>(),
        })
    }
}

/// Immutable, parsed Bible corpora captured from one project data bundle.
///
/// Review reconciliation and rendering share this exact snapshot. A caller
/// supplying reviewed file bytes must prove they still match the parsed
/// corpus, so a changed file cannot be attributed to stale in-memory text.
#[derive(Debug)]
pub struct BibleCorpusSnapshot {
    root: PathBuf,
    corpora: HashMap<BibleVersion, CachedBibleData>,
}

impl BibleCorpusSnapshot {
    /// Parse every installed translation and reject duplicate identities.
    ///
    /// Missing translations are allowed until a plan requests one.
    pub fn capture(root: PathBuf) -> Result<Self, BibleCorpusError> {
        let mut hashes = BTreeMap::<[u8; 32], BibleVersion>::new();
        let mut corpora = HashMap::new();
        for path in corpus_files(&root)? {
            // A file whose name is not a translation code is not a corpus.
            let Some(version) = version::corpus_version(&path) else {
                continue;
            };
            let bytes = read_corpus(version, &path)?;
            let source_sha256: [u8; 32] = Sha256::digest(&bytes).into();
            if let Some(first) = hashes.insert(source_sha256, version) {
                return Err(BibleCorpusError::DuplicateTranslation {
                    first,
                    second: version,
                });
            }
            let data = parse_corpus(version, &path, &bytes)?;
            if corpora
                .insert(
                    version,
                    CachedBibleData {
                        source_sha256,
                        data,
                    },
                )
                .is_some()
            {
                return Err(BibleCorpusError::DuplicateFile { version, path });
            }
        }
        Ok(Self { root, corpora })
    }

    /// Content identity of the exact corpora parsed into this snapshot.
    #[must_use]
    pub fn fingerprint(&self) -> BibleCorpusFingerprint {
        BibleCorpusFingerprint {
            root: self.root.clone(),
            sources: self
                .corpora
                .iter()
                .map(|(version, corpus)| (version.name().to_string(), corpus.source_sha256))
                .collect(),
        }
    }

    /// Verify every installed corpus still matches this snapshot.
    pub fn verify_current(&self) -> Result<(), BibleCorpusError> {
        for (version, corpus) in &self.corpora {
            let path = self.root.join(version.file_name());
            let bytes = read_corpus(*version, &path)?;
            Self::verify_bytes(*version, &path, &bytes, corpus)?;
        }
        Ok(())
    }

    /// Look up individual verses preserving verse boundaries.
    ///
    /// Returns a header (with any missing verse numbers) and a `Vec<Verse>`.
    pub fn lookup_verses(
        &self,
        reference: &ScriptureRef,
        version: BibleVersion,
    ) -> Result<(ScriptureHeader, Vec<Verse>), ScriptureLookupError> {
        self.lookup_captured(reference, version)
    }

    /// Look up verses from exact caller-supplied Bible source bytes.
    ///
    /// The cache is keyed by both version and content hash so a prior build
    /// cannot leak stale translation data into a reviewed build.
    pub fn lookup_reviewed_verses(
        &self,
        reference: &ScriptureRef,
        version: BibleVersion,
        source_bytes: &[u8],
    ) -> Result<(ScriptureHeader, Vec<Verse>), ScriptureLookupError> {
        let corpus = self.corpus(version)?;
        let path = self.root.join(version.file_name());
        Self::verify_bytes(version, &path, source_bytes, corpus)?;
        self.lookup_captured(reference, version)
    }

    fn lookup_captured(
        &self,
        reference: &ScriptureRef,
        version: BibleVersion,
    ) -> Result<(ScriptureHeader, Vec<Verse>), ScriptureLookupError> {
        let bible = &self.corpus(version)?.data;

        let book_data =
            bible
                .get(&reference.book)
                .ok_or_else(|| ScriptureLookupError::BookNotFound {
                    book: reference.book.clone(),
                })?;

        let chapter_data = book_data
            .get(&reference.chapter.to_string())
            .ok_or_else(|| ScriptureLookupError::ChapterNotFound {
                book: reference.book.clone(),
                chapter: reference.chapter,
            })?;

        let requested_verses = match &reference.verses {
            VerseSelection::Chapter => {
                let mut verses = chapter_data
                    .keys()
                    .map(|verse| {
                        verse
                            .parse::<u32>()
                            .map_err(|_| ScriptureLookupError::InvalidVerseKey {
                                book: reference.book.clone(),
                                chapter: reference.chapter,
                                key: verse.clone(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                verses.sort_unstable();
                verses
            }
            VerseSelection::Verse(verse) => vec![*verse],
            VerseSelection::Range { start, end } => (*start..=*end).collect(),
            VerseSelection::Disjoint(ranges) => ranges
                .iter()
                .flat_map(|range| range.start..=range.end)
                .collect(),
        };
        let mut verses = Vec::new();
        let mut missing_verses = Vec::new();

        for verse_num in requested_verses {
            if let Some(text) = chapter_data.get(&verse_num.to_string()) {
                let clean_text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                verses.push(Verse {
                    number: verse_num,
                    text: clean_text,
                });
            } else {
                missing_verses.push(verse_num);
            }
        }

        let header = ScriptureHeader {
            book: reference.book.clone(),
            chapter: reference.chapter,
            verses: reference.verses.clone(),
            version,
            missing_verses,
        };

        Ok((header, verses))
    }

    fn corpus(&self, version: BibleVersion) -> Result<&CachedBibleData, ScriptureLookupError> {
        self.corpora
            .get(&version)
            .ok_or_else(|| ScriptureLookupError::CorpusNotInstalled {
                version,
                path: self.root.join(version.file_name()),
            })
    }

    fn verify_bytes(
        version: BibleVersion,
        path: &std::path::Path,
        bytes: &[u8],
        corpus: &CachedBibleData,
    ) -> Result<(), BibleCorpusError> {
        let actual: [u8; 32] = Sha256::digest(bytes).into();
        if actual == corpus.source_sha256 {
            return Ok(());
        }
        Err(BibleCorpusError::Changed {
            version,
            path: path.to_path_buf(),
            expected: digest_hex(&corpus.source_sha256),
            actual: digest_hex(&actual),
        })
    }
}

/// Every `.json` file directly in `root`, in name order. A data root with no
/// `bibles` directory has no corpora.
fn corpus_files(root: &std::path::Path) -> Result<Vec<PathBuf>, BibleCorpusError> {
    let unreadable = |source| BibleCorpusError::List {
        path: root.to_path_buf(),
        source,
    };
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(unreadable(error)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(unreadable)?.path();
        let is_json = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
        if is_json && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn read_corpus(version: BibleVersion, path: &std::path::Path) -> Result<Vec<u8>, BibleCorpusError> {
    std::fs::read(path).map_err(|source| BibleCorpusError::Read {
        version,
        path: path.to_path_buf(),
        source,
    })
}

fn parse_corpus(
    version: BibleVersion,
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<BibleData, BibleCorpusError> {
    serde_json::from_slice(bytes).map_err(|source| BibleCorpusError::Parse {
        version,
        path: path.to_path_buf(),
        source,
    })
}

fn digest_hex(bytes: &[u8; 32]) -> String {
    let mut digest = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(digest, "{byte:02x}");
    }
    digest
}
