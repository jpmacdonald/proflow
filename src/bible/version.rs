//! Bible translation identity and the set a data root recognizes.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_CODE_LENGTH: usize = 16;

/// One Bible translation, named by its code: `NIV`, `ESV`, `NRSVUE`.
///
/// A code is 2 to 16 ASCII letters and digits, starting with a letter. Letter
/// case does not distinguish translations. A translation's corpus is the file
/// `<data root>/bibles/<CODE>.json`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, schemars::JsonSchema)]
#[schemars(with = "String")]
pub struct BibleVersion {
    /// Upper-case code, padded with zero bytes.
    code: [u8; MAX_CODE_LENGTH],
}

#[allow(non_upper_case_globals)] // `NRSVue` is the translation's own spelling
impl BibleVersion {
    /// New Revised Standard Version Updated Edition.
    pub const NRSVue: Self = Self::known("NRSVUE");
    /// New Revised Standard Version.
    pub const NRSV: Self = Self::known("NRSV");
    /// New International Version.
    pub const NIV: Self = Self::known("NIV");
    /// King James Version.
    pub const KJV: Self = Self::known("KJV");
    /// New King James Version.
    pub const NKJV: Self = Self::known("NKJV");
    /// New Living Translation.
    pub const NLT: Self = Self::known("NLT");
    /// New American Standard Bible.
    pub const NASB: Self = Self::known("NASB");

    /// Translations recognized by name without a corpus being installed.
    const STANDARD: [Self; 7] = [
        Self::NRSVue,
        Self::NRSV,
        Self::NIV,
        Self::KJV,
        Self::NKJV,
        Self::NLT,
        Self::NASB,
    ];

    const fn known(code: &str) -> Self {
        let bytes = code.as_bytes();
        let mut stored = [0; MAX_CODE_LENGTH];
        let mut index = 0;
        while index < bytes.len() {
            stored[index] = bytes[index];
            index += 1;
        }
        Self { code: stored }
    }

    /// Parse a translation code.
    pub fn new(code: &str) -> Result<Self, String> {
        let code = code.trim();
        let valid = (2..=MAX_CODE_LENGTH).contains(&code.len())
            && code.starts_with(|first: char| first.is_ascii_alphabetic())
            && code.bytes().all(|byte| byte.is_ascii_alphanumeric());
        if !valid {
            return Err(format!(
                "'{code}' is not a Bible translation code: 2 to {MAX_CODE_LENGTH} ASCII letters and digits, starting with a letter"
            ));
        }
        let mut stored = [0; MAX_CODE_LENGTH];
        for (stored, byte) in stored.iter_mut().zip(code.bytes()) {
            *stored = byte.to_ascii_uppercase();
        }
        Ok(Self { code: stored })
    }

    /// The translation's name as it is shown and recorded.
    pub fn name(&self) -> &str {
        if *self == Self::NRSVue {
            return "NRSVue";
        }
        let length = self
            .code
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(MAX_CODE_LENGTH);
        // The code holds only ASCII letters and digits by construction.
        std::str::from_utf8(&self.code[..length]).unwrap_or_default()
    }

    /// File name of this translation's corpus below `bibles/`.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.json", self.name().to_ascii_uppercase())
    }
}

impl Default for BibleVersion {
    fn default() -> Self {
        Self::NRSVue
    }
}

impl fmt::Debug for BibleVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "BibleVersion({})", self.name())
    }
}

impl fmt::Display for BibleVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl Serialize for BibleVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for BibleVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// The translations a plan may name.
///
/// A name outside this set in a plan is an unsupported translation and goes
/// to review. The set is the standard translations plus every corpus file in
/// the data root, so installing `bibles/ESV.json` is what makes `ESV` a
/// translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibleTranslations(BTreeSet<BibleVersion>);

impl BibleTranslations {
    /// The translations recognized with no data root.
    #[must_use]
    pub fn standard() -> Self {
        Self(BibleVersion::STANDARD.into_iter().collect())
    }

    /// The standard translations and every `<CODE>.json` in `bibles`.
    ///
    /// A file whose name is not a translation code is not a translation
    /// here; loading the corpora reports it.
    pub fn with_installed(bibles: &Path) -> std::io::Result<Self> {
        let mut translations = Self::standard();
        let entries = match std::fs::read_dir(bibles) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(translations),
            Err(error) => return Err(error),
        };
        for entry in entries {
            if let Some(version) = corpus_version(&entry?.path()) {
                translations.0.insert(version);
            }
        }
        Ok(translations)
    }

    /// The recognized translation called `name`, in any letter case.
    #[must_use]
    pub fn named(&self, name: &str) -> Option<BibleVersion> {
        BibleVersion::new(name)
            .ok()
            .filter(|version| self.0.contains(version))
    }

    /// Every recognized translation, in code order.
    pub fn iter(&self) -> impl Iterator<Item = BibleVersion> + '_ {
        self.0.iter().copied()
    }
}

/// The translation a corpus file holds, by its name: `ESV.json` is `ESV`.
pub(super) fn corpus_version(path: &Path) -> Option<BibleVersion> {
    let is_json = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    is_json
        .then(|| path.file_stem()?.to_str())
        .flatten()
        .and_then(|stem| BibleVersion::new(stem).ok())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_code_is_short_ascii_and_case_does_not_distinguish_translations() {
        let esv = BibleVersion::new("esv").expect("code");
        assert_eq!(esv, BibleVersion::new(" ESV ").expect("code"));
        assert_eq!(esv.name(), "ESV");
        assert_eq!(esv.file_name(), "ESV.json");
        assert_eq!(esv.to_string(), "ESV");
        assert_eq!(
            BibleVersion::new("NASB1995").expect("digits").name(),
            "NASB1995"
        );

        for invalid in [
            "",
            "X",
            "1ESV",
            "NIV commentary",
            "N/V",
            "ÉSV",
            &"A".repeat(17),
        ] {
            assert!(BibleVersion::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn the_updated_edition_keeps_its_own_spelling_and_its_file_name() {
        assert_eq!(
            BibleVersion::new("nrsvue").expect("code"),
            BibleVersion::NRSVue
        );
        assert_eq!(BibleVersion::NRSVue.name(), "NRSVue");
        assert_eq!(BibleVersion::NRSVue.file_name(), "NRSVUE.json");
        assert_eq!(BibleVersion::default(), BibleVersion::NRSVue);
        assert_eq!(
            serde_json::to_string(&BibleVersion::NRSVue).expect("serialize"),
            "\"NRSVue\""
        );
        for spelling in ["\"NRSVue\"", "\"NRSVUE\""] {
            assert_eq!(
                serde_json::from_str::<BibleVersion>(spelling).expect("deserialize"),
                BibleVersion::NRSVue
            );
        }
        assert!(serde_json::from_str::<BibleVersion>("\"New International\"").is_err());
    }

    #[test]
    fn installing_a_corpus_file_makes_its_name_a_translation() {
        let standard = BibleTranslations::standard();
        assert_eq!(standard.named("niv"), Some(BibleVersion::NIV));
        assert_eq!(standard.named("ESV"), None);
        assert_eq!(standard.named("NIV commentary"), None);
        assert_eq!(standard.iter().count(), 7);

        let root = tempfile::tempdir().expect("temporary root");
        let absent = BibleTranslations::with_installed(&root.path().join("bibles"))
            .expect("a data root without corpora");
        assert_eq!(absent, standard);

        for file in ["ESV.json", "csb.JSON", "notes.txt", "not a code.json"] {
            std::fs::write(root.path().join(file), b"{}").expect("write");
        }
        let installed = BibleTranslations::with_installed(root.path()).expect("installed");
        let esv = BibleVersion::new("ESV").expect("code");
        assert_eq!(installed.named("esv"), Some(esv));
        assert!(installed.named("CSB").is_some());
        assert_eq!(installed.named("notes"), None);
        assert_eq!(installed.iter().count(), 9);
    }
}
