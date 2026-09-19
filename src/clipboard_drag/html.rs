//! Compose captured HTML selections without nesting documents or retaining
//! multiple CF_HTML fragment markers. Single-record export bypasses this helper.

const START: &str = "StartFragment";
const END: &str = "EndFragment";

pub(super) fn append(output: &mut String, text: &str, limit: usize) -> Result<(), String> {
    if output.len().saturating_add(text.len()) > limit {
        return Err("clipboard text export exceeds 16 MiB".into());
    }
    output.push_str(text);
    Ok(())
}

/// Preserve the selected source and its structural ancestors (notably table,
/// list and inline styling elements), excluding html/head/body document shells.
pub(super) fn fragment(source: &str, limit: usize) -> Result<String, String> {
    let mut start = None;
    let mut selection = None;
    let mut body = None;
    let mut body_start = None;
    for token in Scanner::new(source) {
        match token.kind {
            Kind::Comment(START) if start.is_none() => start = Some(token.end),
            Kind::Comment(END) if start.is_some() && selection.is_none() => {
                selection = start.map(|start| (start, token.start));
            }
            Kind::Tag {
                name,
                closing: false,
                ..
            } if name.eq_ignore_ascii_case("body") => {
                body_start = Some(token.end);
            }
            Kind::Tag {
                name,
                closing: true,
                ..
            } if name.eq_ignore_ascii_case("body") => {
                body = body_start.map(|start| (start, token.start));
            }
            _ => {}
        }
    }
    let (start, end) = selection.or(body).unwrap_or((0, source.len()));
    let mut ancestors: Vec<Token<'_>> = Vec::new();
    for token in Scanner::new(source).take_while(|token| token.end <= start) {
        update_ancestors(&mut ancestors, token);
    }
    let mut output = String::new();
    for ancestor in &ancestors {
        append(&mut output, &source[ancestor.start..ancestor.end], limit)?;
    }
    let mut position = start;
    for token in Scanner::new(source).filter(|token| token.start >= start && token.end <= end) {
        append(&mut output, &source[position..token.start], limit)?;
        let excluded = match token.kind {
            Kind::Tag { name, .. } => document_shell(name),
            Kind::Comment(START | END) | Kind::Declaration => true,
            _ => false,
        };
        if !excluded {
            append(&mut output, &source[token.start..token.end], limit)?;
        }
        update_ancestors(&mut ancestors, token);
        position = token.end;
    }
    append(&mut output, &source[position..end], limit)?;
    for ancestor in ancestors.iter().rev() {
        if let Kind::Tag { name, .. } = ancestor.kind {
            append(&mut output, "</", limit)?;
            append(&mut output, name, limit)?;
            append(&mut output, ">", limit)?;
        }
    }
    Ok(output)
}

fn document_shell(name: &str) -> bool {
    ["html", "head", "body"]
        .iter()
        .any(|shell| name.eq_ignore_ascii_case(shell))
}

fn update_ancestors<'a>(ancestors: &mut Vec<Token<'a>>, token: Token<'a>) {
    let Kind::Tag {
        name,
        closing,
        empty,
    } = token.kind
    else {
        return;
    };
    if document_shell(name) || empty {
        return;
    }
    if closing {
        if let Some(index) = ancestors.iter().rposition(|ancestor| {
            matches!(ancestor.kind, Kind::Tag { name: open, .. } if name.eq_ignore_ascii_case(open))
        }) {
            ancestors.truncate(index);
        }
    } else {
        ancestors.push(token);
    }
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Tag {
        name: &'a str,
        closing: bool,
        empty: bool,
    },
    Comment(&'a str),
    Declaration,
}

#[derive(Clone, Copy)]
struct Token<'a> {
    start: usize,
    end: usize,
    kind: Kind<'a>,
}

// This scanner retains source bytes; it does not sanitize or execute HTML.
// Quoted attributes and raw-text elements must not be interpreted as markup.
struct Scanner<'a> {
    source: &'a str,
    position: usize,
    raw: Option<&'a str>,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
            raw: None,
        }
    }
}

impl<'a> Iterator for Scanner<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(offset) = self.source[self.position..].find('<') {
            let start = self.position + offset;
            self.position = start + 1;
            let tail = &self.source[start..];
            if self.raw.is_none() && tail.starts_with("<!--") {
                let end = start + 4 + tail[4..].find("-->")?;
                self.position = end + 3;
                return Some(Token {
                    start,
                    end: self.position,
                    kind: Kind::Comment(&self.source[start + 4..end]),
                });
            }
            let closing = tail.starts_with("</");
            let name_start = start + if closing { 2 } else { 1 };
            let mut name_end = name_start;
            while self
                .source
                .as_bytes()
                .get(name_end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b':' | b'-'))
            {
                name_end += 1;
            }
            let declaration = tail.starts_with("<!") || tail.starts_with("<?");
            if name_end == name_start && !declaration {
                continue;
            }
            let name = &self.source[name_start..name_end];
            if self
                .raw
                .is_some_and(|raw| !closing || !name.eq_ignore_ascii_case(raw))
            {
                continue;
            }
            let mut quote = None;
            let mut end = name_end;
            loop {
                let byte = *self.source.as_bytes().get(end)?;
                if quote == Some(byte) {
                    quote = None;
                } else if quote.is_none() {
                    if matches!(byte, b'\'' | b'"') {
                        quote = Some(byte);
                    } else if byte == b'>' {
                        break;
                    }
                }
                end += 1;
            }
            self.position = end + 1;
            if declaration {
                return Some(Token {
                    start,
                    end: self.position,
                    kind: Kind::Declaration,
                });
            }
            let empty = self.source[start..end].trim_end().ends_with('/')
                || [
                    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
                    "param", "source", "track", "wbr",
                ]
                .iter()
                .any(|void| name.eq_ignore_ascii_case(void));
            self.raw = if closing {
                None
            } else {
                ["script", "style", "textarea", "title"]
                    .iter()
                    .any(|raw| name.eq_ignore_ascii_case(raw))
                    .then_some(name)
            };
            return Some(Token {
                start,
                end: self.position,
                kind: Kind::Tag {
                    name,
                    closing,
                    empty,
                },
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_only_marked_content_and_retains_table_ancestors() {
        let source = "<html><body><p>outside</p><table class='样式'><tbody><!--StartFragment--><tr><td>中文</td></tr><!--EndFragment--></tbody></table><p>outside</p></body></html>";
        assert_eq!(
            fragment(source, 1024).unwrap(),
            "<table class='样式'><tbody><tr><td>中文</td></tr></tbody></table>"
        );
    }

    #[test]
    fn full_documents_use_body_and_ignore_fake_markup_in_attributes_and_scripts() {
        let source = "<HTML><head><script>let s = '<body><!--StartFragment-->fake';</script></head><BODY title='a > b'><b>one</b><br><i>two</i></BODY></HTML>";
        assert_eq!(fragment(source, 1024).unwrap(), "<b>one</b><br><i>two</i>");
    }

    #[test]
    fn nested_inline_selection_is_balanced_and_bounded() {
        assert_eq!(
            fragment(
                "<p><b><!--StartFragment-->hello</b> world<!--EndFragment--></p>",
                100
            )
            .unwrap(),
            "<p><b>hello</b> world</p>"
        );
        assert!(fragment("<p><!--StartFragment-->hello<!--EndFragment--></p>", 6).is_err());
    }
}
