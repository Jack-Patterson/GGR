use std::collections::BTreeMap;

use ggr_core::GameError;

/// The localisation registry: every user-facing string is a key resolved here. English only in
/// the demo, but the registry is complete — a second language is a second file.
#[derive(Debug, Clone, Default)]
pub struct Loc {
    language: String,
    strings: BTreeMap<String, String>,
}

impl Loc {
    pub fn parse(text: &str) -> Result<Loc, GameError> {
        #[derive(serde::Deserialize)]
        struct File {
            language: String,
            strings: BTreeMap<String, String>,
        }
        let f: File = serde_yaml::from_str(text)
            .map_err(|e| GameError::content(format!("lang/en.yaml: {e}")))?;
        Ok(Loc {
            language: f.language,
            strings: f.strings,
        })
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn has(&self, key: &str) -> bool {
        self.strings.contains_key(key)
    }

    /// The string for `key`. A missing key renders as the key itself so it is visible in play;
    /// the validator and the UI key scan make that a test failure, never a shipped bug.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).map(String::as_str).unwrap_or(key)
    }

    /// `key` with `{0}`, `{1}`... replaced by `args`, in order.
    pub fn f(&self, key: &str, args: &[&str]) -> String {
        let mut s = self.t(key).to_string();
        for (i, a) in args.iter().enumerate() {
            s = s.replace(&format!("{{{i}}}"), a);
        }
        s
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.strings.keys().map(String::as_str)
    }
}
