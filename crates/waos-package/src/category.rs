//! What an app is for, as its author says it: one of the App Store's categories, which macOS
//! groups its apps by (`LSApplicationCategoryType`) and which map onto the coarser ones Linux's
//! menus use, so one line in a manifest files the app under the same heading wherever it shows.
//! The games' subcategories are left out: a game is under Games.
//!
//! The invariant: a category is spelled as [`Category::as_str`] spells it, Apple's identifier
//! without its `public.app-category.` prefix, and no other spelling reads as one.

use std::fmt;
use std::str::FromStr;

/// What an app is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Business,
    DeveloperTools,
    Education,
    Entertainment,
    Finance,
    Games,
    GraphicsDesign,
    HealthcareFitness,
    Lifestyle,
    Medical,
    Music,
    News,
    Photography,
    Productivity,
    Reference,
    SocialNetworking,
    Sports,
    Travel,
    Utilities,
    Video,
    Weather,
}

impl Category {
    /// Every category, in the order of their spellings.
    pub const ALL: [Category; 21] = [
        Category::Business,
        Category::DeveloperTools,
        Category::Education,
        Category::Entertainment,
        Category::Finance,
        Category::Games,
        Category::GraphicsDesign,
        Category::HealthcareFitness,
        Category::Lifestyle,
        Category::Medical,
        Category::Music,
        Category::News,
        Category::Photography,
        Category::Productivity,
        Category::Reference,
        Category::SocialNetworking,
        Category::Sports,
        Category::Travel,
        Category::Utilities,
        Category::Video,
        Category::Weather,
    ];

    /// The category as a manifest spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Business => "business",
            Category::DeveloperTools => "developer-tools",
            Category::Education => "education",
            Category::Entertainment => "entertainment",
            Category::Finance => "finance",
            Category::Games => "games",
            Category::GraphicsDesign => "graphics-design",
            Category::HealthcareFitness => "healthcare-fitness",
            Category::Lifestyle => "lifestyle",
            Category::Medical => "medical",
            Category::Music => "music",
            Category::News => "news",
            Category::Photography => "photography",
            Category::Productivity => "productivity",
            Category::Reference => "reference",
            Category::SocialNetworking => "social-networking",
            Category::Sports => "sports",
            Category::Travel => "travel",
            Category::Utilities => "utilities",
            Category::Video => "video",
            Category::Weather => "weather",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A spelling that is no category's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownCategory(pub String);

impl fmt::Display for UnknownCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let known: Vec<&str> = Category::ALL
            .iter()
            .map(|category| category.as_str())
            .collect();
        write!(
            f,
            "`{}` is no category; the categories are {}",
            self.0,
            known.join(", ")
        )
    }
}

impl std::error::Error for UnknownCategory {}

impl FromStr for Category {
    type Err = UnknownCategory;

    fn from_str(spelled: &str) -> Result<Category, UnknownCategory> {
        Category::ALL
            .into_iter()
            .find(|category| category.as_str() == spelled)
            .ok_or_else(|| UnknownCategory(spelled.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_reads_back_from_its_spelling_and_nothing_else_does() {
        for category in Category::ALL {
            assert_eq!(category.as_str().parse(), Ok(category));
        }
        assert_eq!(
            "Developer Tools".parse::<Category>(),
            Err(UnknownCategory("Developer Tools".to_owned()))
        );
    }

    #[test]
    fn an_unknown_spelling_is_refused_naming_the_categories() {
        let refusal = "astronomy".parse::<Category>().unwrap_err().to_string();
        assert!(refusal.contains("`astronomy` is no category"), "{refusal}");
        assert!(refusal.contains("developer-tools"), "{refusal}");
    }
}
