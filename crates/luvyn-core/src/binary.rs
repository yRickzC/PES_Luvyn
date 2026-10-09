//! v2: 32-byte header + postcard directory + individually checksummed node records.
use crate::{Error, Result, model::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const MAGIC: &[u8; 8] = b"LUVYN\0\r\n";
const MAX_FILE: u64 = 256 * 1024 * 1024;
const HEADER: u64 = 32;
/// Public format identification for external integrations. Layout is documented in docs/LU_FORMAT.md.
pub const FORMAT_VERSION: u16 = 2;
pub const FORMAT_MAGIC: &[u8; 8] = MAGIC;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub name: String,
    pub qualified: String,
    pub kind: String,
    pub offset: u64,
    pub length: u32,
    pub checksum: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Directory {
    pub records: Vec<Record>,
    pub edges: Vec<Edge>,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: BTreeMap<String, String>,
    pub text_index: BTreeMap<String, Vec<usize>>,
}
pub struct Artifact {
    file: File,
    pub directory: Directory,
    data_start: u64,
    data_length: u64,
    by_id: std::collections::HashMap<String, usize>,
}
fn corrupt(message: &str) -> Error {
    Error::Message(format!("Invalid .lu artifact: {message}; run luvyn build"))
}
pub fn encode(graph: &Graph) -> Result<Vec<u8>> {
    let mut directory = Directory {
        records: Vec::new(),
        edges: graph.edges.clone(),
        diagnostics: graph.diagnostics.clone(),
        sources: graph.sources.clone(),
        text_index: graph
            .text_index
            .iter()
            .map(|(word, indices)| (word.clone(), indices.clone()))
            .collect(),
    };
    let mut data = Vec::new();
    for symbol in &graph.symbols {
        let record = postcard::to_allocvec(symbol).map_err(|e| Error::Message(e.to_string()))?;
        directory.records.push(Record {
            id: symbol.id.clone(),
            name: symbol.name.clone(),
            qualified: symbol.qualified.clone(),
            kind: symbol.kind.clone(),
            offset: data.len() as u64,
            length: record.len() as u32,
            checksum: crc32fast::hash(&record),
        });
        data.extend(record);
    }
    let index = postcard::to_allocvec(&directory).map_err(|e| Error::Message(e.to_string()))?;
    let mut out = Vec::with_capacity(32 + index.len() + data.len());
    out.extend(MAGIC);
    out.extend(FORMAT_VERSION.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend((index.len() as u32).to_le_bytes());
    out.extend((data.len() as u64).to_le_bytes());
    out.extend(crc32fast::hash(&index).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(index);
    out.extend(data);
    if out.len() as u64 > MAX_FILE {
        return Err(Error::Message(
            "Compiled graph exceeds 256 MiB; split workspace".into(),
        ));
    }
    Ok(out)
}
pub fn write(path: &Path, graph: &Graph) -> Result<()> {
    crate::workspace::atomic_write(path, &encode(graph)?)
}
impl Artifact {
    /// Stable adapter API: enumerate metadata without loading node bodies.
    pub fn nodes(&self) -> &[Record] {
        &self.directory.records
    }
    pub fn edges(&self) -> &[Edge] {
        &self.directory.edges
    }
    pub fn source_hashes(&self) -> &BTreeMap<String, String> {
        &self.directory.sources
    }
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        if !(HEADER..=MAX_FILE).contains(&length) {
            return Err(corrupt("invalid file size"));
        }
        let mut header = [0u8; 32];
        file.read_exact(&mut header)?;
        if &header[..8] != MAGIC {
            return Err(corrupt("wrong magic"));
        }
        let u16at = |i| u16::from_le_bytes([header[i], header[i + 1]]);
        let u32at =
            |i| u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
        if u16at(8) != FORMAT_VERSION || u16at(10) != 0 || u32at(28) != 0 {
            return Err(corrupt("unsupported version or flags"));
        }
        let index_length = u32at(12) as u64;
        let data_length =
            u64::from_le_bytes(header[16..24].try_into().map_err(|_| corrupt("header"))?);
        if index_length > 64 * 1024 * 1024
            || HEADER
                .checked_add(index_length)
                .and_then(|n| n.checked_add(data_length))
                != Some(length)
        {
            return Err(corrupt("section bounds"));
        }
        let mut index = vec![0u8; index_length as usize];
        file.read_exact(&mut index)?;
        if crc32fast::hash(&index) != u32at(24) {
            return Err(corrupt("directory checksum"));
        }
        let directory: Directory =
            postcard::from_bytes(&index).map_err(|_| corrupt("directory encoding"))?;
        let mut ids = std::collections::HashSet::new();
        let mut previous = 0;
        for record in &directory.records {
            if record.offset != previous
                || record.length as u64 > 4 * 1024 * 1024
                || record
                    .offset
                    .checked_add(record.length as u64)
                    .is_none_or(|end| end > data_length)
                || !ids.insert(record.id.clone())
            {
                return Err(corrupt("node record bounds or duplicate ID"));
            }
            previous = record.offset + record.length as u64;
        }
        if previous != data_length
            || directory
                .edges
                .iter()
                .any(|e| !ids.contains(&e.from) || !ids.contains(&e.to))
        {
            return Err(corrupt("dangling edges or trailing records"));
        }
        if directory
            .text_index
            .values()
            .flatten()
            .any(|i| *i >= directory.records.len())
        {
            return Err(corrupt("text index bounds"));
        }
        let by_id = directory
            .records
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.clone(), i))
            .collect();
        Ok(Self {
            file,
            directory,
            data_start: HEADER + index_length,
            data_length,
            by_id,
        })
    }
    /// Direct node access: other node payloads are never deserialized.
    pub fn node(&mut self, id: &str) -> Result<Option<Symbol>> {
        let Some(record) = self
            .by_id
            .get(id)
            .and_then(|i| self.directory.records.get(*i))
        else {
            return Ok(None);
        };
        if record.offset + record.length as u64 > self.data_length {
            return Err(corrupt("node bounds"));
        }
        self.file
            .seek(SeekFrom::Start(self.data_start + record.offset))?;
        let mut bytes = vec![0u8; record.length as usize];
        self.file.read_exact(&mut bytes)?;
        if crc32fast::hash(&bytes) != record.checksum {
            return Err(corrupt("node checksum"));
        }
        let symbol: Symbol = postcard::from_bytes(&bytes).map_err(|_| corrupt("node encoding"))?;
        if symbol.id != record.id
            || symbol.qualified != record.qualified
            || symbol.kind != record.kind
            || stable_id(&symbol.qualified, &symbol.kind) != symbol.id
        {
            return Err(corrupt("node identity"));
        }
        Ok(Some(symbol))
    }
    pub fn load(mut self) -> Result<Graph> {
        let ids: Vec<_> = self
            .directory
            .records
            .iter()
            .map(|r| r.id.clone())
            .collect();
        let mut symbols = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(s) = self.node(&id)? {
                symbols.push(s);
            }
        }
        let mut graph = Graph {
            symbols,
            edges: self.directory.edges,
            diagnostics: self.directory.diagnostics,
            sources: self.directory.sources,
            ..Default::default()
        };
        graph.index();
        Ok(graph)
    }
    /// Query directory/index first, then deserialize only selected node records.
    pub fn query(
        &mut self,
        input: &str,
        options: &crate::query::QueryOptions,
    ) -> Result<crate::query::QueryResult> {
        let symbols = self
            .directory
            .records
            .iter()
            .map(|r| Symbol {
                id: r.id.clone(),
                name: r.name.clone(),
                qualified: r.qualified.clone(),
                kind: r.kind.clone(),
                namespace: String::new(),
                parent: None,
                signature: None,
                annotations: vec![],
                generics: vec![],
                sections: BTreeMap::new(),
                location: Location::default(),
                end_line: 0,
            })
            .collect();
        let mut graph = Graph {
            symbols,
            edges: self.directory.edges.clone(),
            ..Default::default()
        };
        graph.index();
        graph.text_index = self
            .directory
            .text_index
            .iter()
            .map(|(w, v)| (w.clone(), v.clone()))
            .collect();
        let mut result = crate::query::query(&graph, input, options)?;
        for symbol in &mut result.symbols {
            *symbol = self
                .node(&symbol.id)?
                .ok_or_else(|| corrupt("missing selected node"))?;
        }
        Ok(result)
    }
}
pub fn read(path: &Path) -> Result<Graph> {
    Artifact::open(path)?.load()
}
