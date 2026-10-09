//! Shared lexical classifications with UTF-16 offsets for desktop and touch editors.
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct Token {
    pub from: u32,
    pub to: u32,
    pub kind: &'static str,
}
pub fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut base = 0;
    for line in source.split_inclusive('\n') {
        let code = crate::parser::strip_comment(line);
        let chars: Vec<char> = code.chars().collect();
        let mut i = 0;
        let mut units = base;
        while i < chars.len() {
            let start = i;
            let from = units;
            let c = chars[i];
            let kind = if c == '@' {
                i += 1;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '.'))
                {
                    i += 1;
                }
                "annotation"
            } else if matches!(c, '\'' | '"') {
                i += 1;
                while i < chars.len() {
                    let next = chars[i];
                    i += 1;
                    if next == '\\' {
                        i = (i + 1).min(chars.len());
                    } else if next == c {
                        break;
                    }
                }
                "string"
            } else if c.is_alphabetic() || c == '_' {
                i += 1;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if crate::language::LITERALS.contains(&word.as_str()) {
                    "literal"
                } else if crate::parser::builtin(&word) {
                    "type"
                } else if crate::language::lookup(&word).is_some() {
                    "keyword"
                } else {
                    "identifier"
                }
            } else if c.is_ascii_digit() {
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_')
                {
                    i += 1;
                }
                "number"
            } else if (c == '-' && chars.get(i + 1) == Some(&'>'))
                || (c == ':' && chars.get(i + 1) == Some(&':'))
            {
                i += 2;
                "operator"
            } else {
                i += 1;
                if ".:<>?=+*/!|-".contains(c) {
                    "operator"
                } else {
                    "delimiter"
                }
            };
            units += chars[start..i]
                .iter()
                .map(|c| c.len_utf16() as u32)
                .sum::<u32>();
            if !c.is_whitespace() {
                tokens.push(Token {
                    from,
                    to: units,
                    kind,
                });
            }
        }
        if code.len() != line.len() {
            tokens.push(Token {
                from: units,
                to: base + line.encode_utf16().count() as u32,
                kind: "comment",
            });
        }
        base += line.encode_utf16().count() as u32;
    }
    tokens
}
