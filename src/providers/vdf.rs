//! Minimal parser for Valve's KeyValues text format (`.vdf` / `.acf` files):
//! nested `"key" "value"` pairs and `"key" { ... }` blocks.

pub enum Value {
    Str(String),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// Looks up a child by key; VDF keys are case-insensitive.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries().iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Obj(entries) => entries,
            Value::Str(_) => &[],
        }
    }
}

/// Parses a whole document into an object holding its top-level entries.
/// Malformed input is parsed as far as it makes sense rather than rejected.
pub fn parse(text: &str) -> Value {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    Value::Obj(Parser { chars: text.chars().peekable() }.object())
}

enum Token {
    Str(String),
    Open,
    Close,
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Parser<'_> {
    /// Reads entries until the closing brace of the current block (or end of input).
    fn object(&mut self) -> Vec<(String, Value)> {
        let mut entries = Vec::new();
        loop {
            let Some(Token::Str(key)) = self.next_token() else { break };
            let value = match self.next_token() {
                Some(Token::Str(value)) => Value::Str(value),
                Some(Token::Open) => Value::Obj(self.object()),
                _ => break,
            };
            entries.push((key, value));
        }
        entries
    }

    fn next_token(&mut self) -> Option<Token> {
        loop {
            match self.chars.next()? {
                c if c.is_whitespace() => {}
                '{' => return Some(Token::Open),
                '}' => return Some(Token::Close),
                '"' => return Some(Token::Str(self.quoted())),
                '/' if self.chars.peek() == Some(&'/') => {
                    // Line comment.
                    for c in self.chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                c => return Some(Token::Str(self.unquoted(c))),
            }
        }
    }

    fn quoted(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.chars.next() {
            match c {
                '"' => break,
                '\\' => match self.chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some(c @ ('\\' | '"')) => s.push(c),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => break,
                },
                c => s.push(c),
            }
        }
        s
    }

    fn unquoted(&mut self, first: char) -> String {
        let mut s = String::from(first);
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() || matches!(c, '"' | '{' | '}') {
                break;
            }
            s.push(c);
            self.chars.next();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_app_manifest() {
        let doc = parse(
            r#""AppState"
{
	"appid"		"570"
	"name"		"Dota 2"
	"StateFlags"		"4"
	"UserConfig"
	{
		"language"		"english"
	}
}"#,
        );
        let app = doc.get("appstate").unwrap();
        assert_eq!(app.get_str("appid"), Some("570"));
        assert_eq!(app.get_str("name"), Some("Dota 2"));
        assert_eq!(app.get("UserConfig").unwrap().get_str("language"), Some("english"));
    }

    #[test]
    fn unescapes_paths_and_skips_comments() {
        let doc = parse(
            r#"// header comment
"libraryfolders" { "0" { "path" "D:\\SteamLibrary" } }"#,
        );
        let folder = doc.get("libraryfolders").unwrap().get("0").unwrap();
        assert_eq!(folder.get_str("path"), Some(r"D:\SteamLibrary"));
    }
}
