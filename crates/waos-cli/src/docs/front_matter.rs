//! A page's front matter, the YAML between its first two `---` lines, as the site's pages write it:
//! a `key: value` a line, the value plain or in double quotes.
//!
//! The invariant: this reads only what the pages use, a flat map of strings. A page whose front
//! matter needs more of YAML than that is read wrong here, so the site's pages keep to it.

/// A page split into the fields its front matter names and the body after it.
#[derive(Debug, PartialEq, Eq)]
pub struct FrontMatter<'a> {
    pub fields: Vec<(&'a str, String)>,
    pub body: &'a str,
}

impl<'a> FrontMatter<'a> {
    /// `text` read as front matter and a body; a text with no front matter is all body.
    pub fn read(text: &'a str) -> FrontMatter<'a> {
        let none = FrontMatter {
            fields: Vec::new(),
            body: text,
        };
        let Some(rest) = text.strip_prefix("---\n") else {
            return none;
        };
        let Some(end) = rest
            .find("\n---\n")
            .or_else(|| rest.strip_suffix("\n---").map(str::len))
        else {
            return none;
        };
        let fields = rest[..end].lines().filter_map(field).collect();
        let body = rest[end..].strip_prefix("\n---").unwrap_or("");
        FrontMatter {
            fields,
            body: body.strip_prefix('\n').unwrap_or(body),
        }
    }

    /// The value of `key`, if the front matter names it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// One `key: value` line, its value without the quotes around it.
fn field(line: &str) -> Option<(&str, String)> {
    let (key, value) = line.split_once(':')?;
    let value = value.trim();
    let unquoted = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(quoted) => quoted.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => value.to_owned(),
    };
    Some((key.trim(), unquoted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_read_plain_or_quoted_and_the_body_follows() {
        let page = "---\ntitle: Waft\ndescription: \"The UI framework: Flutter's, in Rust.\"\n---\nWaft is.\n";
        let read = FrontMatter::read(page);
        assert_eq!(read.get("title"), Some("Waft"));
        assert_eq!(
            read.get("description"),
            Some("The UI framework: Flutter's, in Rust.")
        );
        assert_eq!(read.body, "Waft is.\n");
    }

    #[test]
    fn a_text_without_front_matter_is_all_body() {
        let read = FrontMatter::read("# Build an app on WaOS\n");
        assert_eq!(read.fields, []);
        assert_eq!(read.body, "# Build an app on WaOS\n");
        assert_eq!(
            FrontMatter::read("---\ntitle: open\n").body,
            "---\ntitle: open\n"
        );
    }
}
