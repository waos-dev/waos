//! The variables a development run adds to a program's environment, as `--env NAME=VALUE` spells
//! each: the name, then everything after the first `=`, which a value may hold more of (the
//! design's decision 68).
//!
//! The invariant: a variable has a name, and the first `=` is the one that ends it, so a value
//! may hold `=` and an empty one is a value of nothing.

/// The name and the value `spec` spells as `NAME=VALUE`; why not, for one with no `=`, or none
/// before it.
pub(crate) fn parse(spec: &str) -> Result<(String, String), String> {
    let Some((name, value)) = spec.split_once('=') else {
        return Err("a variable is NAME=VALUE, such as RUST_LOG=debug".to_owned());
    };
    if name.is_empty() {
        return Err("a variable needs a name before its `=`, as in RUST_LOG=debug".to_owned());
    }
    Ok((name.to_owned(), value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_is_the_name_before_the_first_equals_and_the_value_after() {
        assert_eq!(
            parse("RUST_LOG=debug"),
            Ok(("RUST_LOG".to_owned(), "debug".to_owned()))
        );
        assert_eq!(
            parse("API_URL=https://example.test/?a=b"),
            Ok(("API_URL".to_owned(), "https://example.test/?a=b".to_owned())),
            "the first `=` ends the name, and the value may hold more"
        );
        assert_eq!(
            parse("EMPTY="),
            Ok(("EMPTY".to_owned(), String::new())),
            "a value of nothing"
        );
    }

    #[test]
    fn a_variable_with_no_equals_or_no_name_is_refused() {
        let no_equals = parse("RUST_LOG").unwrap_err();
        assert!(no_equals.contains("NAME=VALUE"), "{no_equals}");
        let no_name = parse("=debug").unwrap_err();
        assert!(no_name.contains("needs a name"), "{no_name}");
    }
}
