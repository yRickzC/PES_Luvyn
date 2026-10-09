use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: String,
    pub line: u32,
    /// One-based Unicode scalar column. Editor services convert to UTF-16.
    pub column: u32,
    pub length: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub location: Location,
    pub suggestion: Option<String>,
}
impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>, location: Location) -> Self {
        Self {
            severity: "error".into(),
            code: code.into(),
            message: message.into(),
            location,
            suggestion: None,
        }
    }
    pub fn hint(mut self, text: impl Into<String>) -> Self {
        self.suggestion = Some(text.into());
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reference {
    pub target: String,
    pub relation: String,
    pub location: Location,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Import {
    pub target: String,
    pub alias: Option<String>,
    pub location: Location,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Annotation {
    pub name: String,
    /// Documentary arguments, preserving spelling and quoted values. Never evaluated.
    pub arguments: Vec<String>,
    pub location: Location,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenericParameter {
    pub name: String,
    pub bound: Option<String>,
    pub location: Location,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedSymbol {
    pub kind: String,
    pub name: String,
    pub parent: Option<String>,
    pub signature: Option<String>,
    pub annotations: Vec<Annotation>,
    pub generics: Vec<GenericParameter>,
    pub sections: BTreeMap<String, Vec<String>>,
    pub references: Vec<Reference>,
    pub location: Location,
    pub end_line: u32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ParsedFile {
    pub path: String,
    pub namespace: String,
    pub imports: Vec<Import>,
    pub symbols: Vec<ParsedSymbol>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Symbol {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub qualified: String,
    pub namespace: String,
    pub parent: Option<String>,
    pub signature: Option<String>,
    pub annotations: Vec<Annotation>,
    pub generics: Vec<GenericParameter>,
    pub sections: BTreeMap<String, Vec<String>>,
    pub location: Location,
    pub end_line: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub location: Location,
}
impl PartialOrd for Location {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Location {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.file, self.line, self.column, self.length).cmp(&(
            &other.file,
            other.line,
            other.column,
            other.length,
        ))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Graph {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: BTreeMap<String, String>,
    #[serde(skip)]
    pub by_id: HashMap<String, usize>,
    #[serde(skip)]
    pub by_name: HashMap<String, Vec<usize>>,
    #[serde(skip)]
    pub outgoing: HashMap<String, Vec<usize>>,
    #[serde(skip)]
    pub incoming: HashMap<String, Vec<usize>>,
    #[serde(skip)]
    pub text_index: HashMap<String, Vec<usize>>,
}
impl Graph {
    pub fn index(&mut self) {
        self.by_id.clear();
        self.by_name.clear();
        self.outgoing.clear();
        self.incoming.clear();
        self.text_index.clear();
        for (i, s) in self.symbols.iter().enumerate() {
            self.by_id.insert(s.id.clone(), i);
            for name in [s.name.to_lowercase(), s.qualified.to_lowercase()] {
                let entry = self.by_name.entry(name).or_default();
                if !entry.contains(&i) {
                    entry.push(i);
                }
            }
            let content = format!(
                "{} {} {}",
                s.qualified,
                s.signature.as_deref().unwrap_or(""),
                s.sections
                    .values()
                    .flatten()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let content = format!(
                "{content} {}",
                s.annotations
                    .iter()
                    .map(|a| format!("@{} {}", a.name, a.arguments.join(" ")))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let mut words = words(&content);
            words.sort();
            words.dedup();
            for word in words {
                self.text_index.entry(word).or_default().push(i);
            }
            for key in s.sections.keys() {
                self.text_index
                    .entry(format!("__section:{key}"))
                    .or_default()
                    .push(i);
            }
        }
        for (i, e) in self.edges.iter().enumerate() {
            self.outgoing.entry(e.from.clone()).or_default().push(i);
            self.incoming.entry(e.to.clone()).or_default().push(i);
        }
    }
    pub fn symbol(&self, id: &str) -> Option<&Symbol> {
        self.by_id.get(id).and_then(|i| self.symbols.get(*i))
    }
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == "error")
    }
}
pub fn words(text: &str) -> Vec<String> {
    normalized_text(text)
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
pub fn normalized_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ã' | 'â' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            _ => c,
        })
        .collect()
}
pub fn stable_id(qualified: &str, kind: &str) -> String {
    blake3::hash(format!("luvyn:v1:{kind}:{qualified}").as_bytes()).to_hex()[..24].to_string()
}
