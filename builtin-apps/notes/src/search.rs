//! Searching the notes and the files open: one is found when its texts, a note's text with its
//! title among it, or a file's name and text, hold every word searched for, in any order and
//! whatever their case. An empty search finds every one.

/// What is searched for, as the words it is made of.
#[derive(Debug, Default)]
pub(crate) struct Search {
    /// The words, in lower case.
    words: Vec<String>,
}

impl Search {
    /// The search typed as `typed`.
    pub(crate) fn new(typed: &str) -> Search {
        Search {
            words: typed.split_whitespace().map(str::to_lowercase).collect(),
        }
    }

    /// Whether every word is in one of `texts`.
    pub(crate) fn finds(&self, texts: &[&str]) -> bool {
        if self.words.is_empty() {
            return true;
        }
        let texts: Vec<String> = texts.iter().map(|text| text.to_lowercase()).collect();
        self.words
            .iter()
            .all(|word| texts.iter().any(|text| text.contains(word.as_str())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_finds_its_words_in_the_title_or_the_text_whatever_their_case() {
        let shopping = ["Shopping\nMilk, eggs"];
        assert!(Search::new("shop").finds(&shopping), "in the title");
        assert!(Search::new("EGGS").finds(&shopping), "in the text");
        assert!(
            Search::new("  eggs   milk ").finds(&shopping),
            "in any order"
        );
    }

    #[test]
    fn a_search_finds_its_words_across_a_files_name_and_text() {
        let todo = ["todo.txt", "milk, eggs"];
        assert!(Search::new("todo milk").finds(&todo));
        assert!(!Search::new("todo bread").finds(&todo));
    }

    #[test]
    fn a_search_misses_a_note_without_every_word() {
        let shopping = ["Shopping\nmilk, eggs"];
        assert!(!Search::new("bread").finds(&shopping));
        assert!(!Search::new("milk bread").finds(&shopping));
    }

    #[test]
    fn an_empty_search_finds_every_note() {
        assert!(Search::new("").finds(&[""]));
        assert!(Search::new(" \t").finds(&["Shopping"]));
    }
}
